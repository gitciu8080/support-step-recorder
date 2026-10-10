#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

//! Interface egui/glow de Support Step Recorder.
//!
//! # Community & support
//!
//! Questions, bugs, beta testing — join the Discord: <https://discord.gg/T37DYHmt2j>

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, TryRecvError};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use eframe::egui;
use ssr_core::{export, session::Session, Lang, Progress, Recorder};

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([900.0, 640.0]),
        // Rendu via glow (OpenGL) — explicite.
        renderer: eframe::Renderer::Glow,
        ..Default::default()
    };
    eframe::run_native(
        "Support Step Recorder",
        options,
        Box::new(|cc| {
            install_chinese_font(&cc.egui_ctx);
            Ok(Box::new(App::new()))
        }),
    )
}

/// Ligne d'étape extraite de la session pour l'affichage (sans verrou).
///
/// On garde l'action et la fenêtre brutes (pas le texte déjà traduit) pour
/// pouvoir re-construire la description dans la langue courante à l'affichage.
#[derive(Clone)]
struct StepRow {
    index: usize,
    screenshot: Option<String>,
    /// Horodatage absolu de la capture (ms depuis l'epoch Unix).
    timestamp_ms: u128,
    /// Temps écoulé depuis le début de la session (ms).
    elapsed_ms: u128,
    action: ssr_core::Action,
    window: Option<ssr_core::WindowInfo>,
    step: ssr_core::Step,
}

impl StepRow {
    /// Description lisible dans la langue donnée.
    fn description(&self, lang: Lang) -> String {
        self.step.display_description(lang)
    }
}

/// Instantané de la session pour un rendu de frame.
#[derive(Clone)]
struct Snapshot {
    dir: PathBuf,
    rows: Vec<StepRow>,
}

struct App {
    base_dir: PathBuf,
    screen: Option<Arc<ssr_core::capture::Screen>>,
    screen_ready: bool,
    selecting_screen: bool,
    recorder: Option<Recorder>,
    /// Dernière session terminée, conservée pour la revue et l'export.
    last: Option<Arc<Mutex<Session>>>,
    selected: Option<usize>,
    status: String,
    textures: HashMap<String, egui::TextureHandle>,
    /// Action longue en cours (capture, sauvegarde, compression…), partagée
    /// avec les threads de travail et affichée dans la barre de statut.
    progress: Progress,
    /// Export en cours dans un thread dédié : reçoit le résultat final.
    export: Option<Receiver<Result<PathBuf, String>>>,
    /// Dernier instantané rendu, conservé quand la session est momentanément
    /// verrouillée (par ex. pendant l'export) pour éviter tout clignotement.
    snapshot: Option<Snapshot>,
    /// Session arrêtée en attente d'une destination de sauvegarde.
    pending_save: Option<Arc<Mutex<Session>>>,
    /// Affiche la confirmation « session non sauvegardée » (après annulation).
    confirm_discard: bool,
    /// Langue de l'interface (et des rapports générés).
    lang: Lang,
    analysis: Option<Receiver<Result<usize, String>>>,
    cancel_analysis: Arc<std::sync::atomic::AtomicBool>,
    original_size: bool,
    show_before: bool,
    edit_text: String,
    edit_index: Option<usize>,
    screenshot_quality: f32,
    vision_config: ssr_core::vision::VisionConfig,
    auto_analyze: bool,
}

impl App {
    fn new() -> Self {
        let lang = Lang::detect();
        let mut app = Self {
            screen: None,
            screen_ready: false,
            selecting_screen: false,
            // Les captures sont volatiles : elles vivent dans un dossier temporaire
            // jusqu'à ce que l'utilisateur choisisse où enregistrer le rapport.
            base_dir: std::env::temp_dir().join("support-step-recorder"),
            recorder: None,
            last: None,
            selected: None,
            status: lang.pick("Prêt.", "Ready.").to_string(),
            textures: HashMap::new(),
            progress: Progress::new(),
            export: None,
            snapshot: None,
            pending_save: None,
            confirm_discard: false,
            lang,
            analysis: None,
            cancel_analysis: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            original_size: false,
            show_before: false,
            edit_text: String::new(),
            edit_index: None,
            screenshot_quality: 100.0,
            auto_analyze: ssr_core::vision::VisionConfig::load().is_ok(),
            vision_config: ssr_core::vision::VisionConfig::load().unwrap_or(
                ssr_core::vision::VisionConfig {
                    base_url: String::new(),
                    api_key: String::new(),
                    model: "Qwen3.6-35B-A3B".into(),
                    timeout_seconds: 120,
                },
            ),
        };
        app.select_screen();
        app
    }

    fn select_screen(&mut self) {
        self.screen = None;
        self.screen_ready = false;
        self.selecting_screen = true;
        self.status = self
            .t(
                "Choisissez une fenêtre ou un écran…",
                "Choose a window or screen…",
            )
            .into();
        match ssr_core::capture::Screen::prepare_selection() {
            Ok(screen) => self.screen = Some(Arc::new(screen)),
            Err(error) => {
                self.selecting_screen = false;
                self.status = format!(
                    "{} {error}",
                    self.t(
                        "Sélection annulée ou indisponible :",
                        "Selection cancelled or unavailable:"
                    )
                );
            }
        }
    }

    fn poll_screen(&mut self) {
        let Some(screen) = &self.screen else { return };
        match screen.selection_ready() {
            Ok(true) => {
                if self.selecting_screen {
                    self.status = self
                        .t(
                            "Zone sélectionnée. Prêt à enregistrer.",
                            "Area selected. Ready to record.",
                        )
                        .into();
                }
                self.screen_ready = true;
                self.selecting_screen = false;
            }
            Ok(false) => self.screen_ready = false,
            Err(error) => {
                self.screen_ready = false;
                self.selecting_screen = false;
                self.screen = None;
                self.status = format!(
                    "{} {error}",
                    self.t(
                        "Sélection annulée ou indisponible :",
                        "Selection cancelled or unavailable:"
                    )
                );
            }
        }
    }

    /// Raccourci de traduction : renvoie la chaîne selon la langue courante.
    fn t(&self, fr: &'static str, en: &'static str) -> &'static str {
        self.lang.pick(fr, en)
    }

    /// Change la langue de l'interface.
    fn set_lang(&mut self, lang: Lang) {
        self.lang = lang;
        // Rafraîchit le statut au repos dans la nouvelle langue.
        if !self.is_recording() && self.export.is_none() {
            self.status = self.t("Prêt.", "Ready.").to_string();
        }
    }

    fn is_recording(&self) -> bool {
        self.recorder.is_some()
    }

    /// Session active : celle en cours d'enregistrement, sinon la dernière terminée.
    fn current(&self) -> Option<Arc<Mutex<Session>>> {
        self.recorder
            .as_ref()
            .map(|r| r.session.clone())
            .or_else(|| self.last.clone())
    }

    fn start(&mut self) {
        // Purge le dossier temporaire de la session précédente (les captures
        // n'ont d'intérêt qu'une fois le rapport enregistré par l'utilisateur).
        let Some(screen) = self.screen.clone().filter(|_| self.screen_ready) else {
            return;
        };
        self.cleanup_previous_temp();

        let name = format!("session-{}", chrono::Local::now().format("%Y%m%d-%H%M%S"));
        let dir = self.base_dir.join(name);
        match Recorder::start_with_screen(
            dir.clone(),
            self.progress.clone(),
            self.lang,
            self.screenshot_quality,
            screen,
        ) {
            Ok(rec) => {
                self.status = format!("{}…", self.t("Enregistrement", "Recording"));
                self.recorder = Some(rec);
                self.last = None;
                self.selected = None;
                self.textures.clear();
            }
            Err(err) => {
                self.status = format!("{} {err}", self.t("Échec du démarrage :", "Start failed:"))
            }
        }
    }

    fn stop(&mut self) {
        if let Some(rec) = self.recorder.take() {
            let session = rec.stop();
            let n = session.lock().map(|s| s.steps.len()).unwrap_or(0);
            self.status = format!(
                "{} {n} {}",
                self.t("Enregistrement arrêté —", "Recording stopped —"),
                self.t("étape(s).", "step(s).")
            );
            self.last = Some(session.clone());
            // Enchaîne directement sur le choix de la destination.
            self.pending_save = Some(session);
            if self.auto_analyze {
                self.begin_analysis();
                if self.analysis.is_some() {
                    return;
                }
            }
            self.begin_save();
        }
    }

    /// Ouvre le dialogue « Enregistrer sous » puis lance la génération à
    /// l'emplacement choisi. Annuler déclenche la confirmation.
    fn begin_save(&mut self) {
        let Some(session) = self.pending_save.clone() else {
            return;
        };
        let default = default_zip_name(&session);
        let dest = rfd::FileDialog::new()
            .set_title(self.t(
                "Enregistrer le rapport (ZIP + HTML)",
                "Save report (ZIP + HTML)",
            ))
            .set_file_name(default)
            .add_filter(self.t("Archive ZIP", "ZIP archive"), &["zip"])
            .save_file();
        match dest {
            Some(path) => {
                self.confirm_discard = false;
                self.pending_save = None;
                self.export_to(session, path);
            }
            None => self.confirm_discard = true, // annulation → demander confirmation
        }
    }

    /// Génère le ZIP+HTML vers `path` dans un thread dédié.
    fn export_to(&mut self, session: Arc<Mutex<Session>>, mut path: PathBuf) {
        if path.extension().is_none() {
            path.set_extension("zip");
        }
        let progress = self.progress.clone();
        let lang = self.lang;
        let unreachable = self
            .t("Session inaccessible.", "Session unavailable.")
            .to_string();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let result = match session.lock() {
                Ok(guard) => export::export_zip(&guard, &path, &progress, lang)
                    .map(|()| path)
                    .map_err(|err| err.to_string()),
                Err(_) => Err(unreachable),
            };
            progress.clear();
            let _ = tx.send(result);
        });
        self.status = self
            .t("Génération du rapport…", "Generating report…")
            .to_string();
        self.export = Some(rx);
    }

    /// Supprime le dossier temporaire de la dernière session.
    fn cleanup_previous_temp(&mut self) {
        if let Some(prev) = self.last.take() {
            if let Ok(s) = prev.lock() {
                let _ = std::fs::remove_dir_all(&s.dir);
            }
        }
    }

    /// Récupère le résultat de l'export terminé, s'il y en a un.
    fn poll_export(&mut self) {
        let Some(rx) = &self.export else { return };
        match rx.try_recv() {
            Ok(Ok(path)) => {
                self.status = format!("{} {}", self.t("Exporté :", "Exported:"), path.display());
                self.export = None;
            }
            Ok(Err(err)) => {
                self.status = format!("{} {err}", self.t("Échec de l'export :", "Export failed:"));
                self.export = None;
            }
            Err(TryRecvError::Empty) => {} // toujours en cours
            Err(TryRecvError::Disconnected) => {
                self.status = self
                    .t("Export interrompu.", "Export interrupted.")
                    .to_string();
                self.export = None;
            }
        }
    }

    fn begin_analysis(&mut self) {
        let Some(session) = self.last.clone() else {
            return;
        };
        let config = match ssr_core::vision::VisionConfig::load() {
            Ok(config) => config,
            Err(error) => {
                self.status = error;
                return;
            }
        };
        self.cancel_analysis = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let cancel = self.cancel_analysis.clone();
        let progress = self.progress.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let result =
                ssr_core::vision::analyze_session(session, config, cancel, progress.clone());
            progress.clear();
            let _ = tx.send(result);
        });
        self.analysis = Some(rx);
        self.status = "正在生成中文步骤描述…".into();
    }

    fn poll_analysis(&mut self) {
        let Some(rx) = &self.analysis else {
            return;
        };
        match rx.try_recv() {
            Ok(result) => {
                self.status = match result {
                    Ok(count) => format!("本次已生成 {count} 条描述；可检查、修改并重新导出。"),
                    Err(error) => error,
                };
                self.analysis = None;
                if self.pending_save.is_some() {
                    self.begin_save();
                }
            }
            Err(TryRecvError::Disconnected) => {
                self.status = "视觉分析已中断".into();
                self.analysis = None;
                if self.pending_save.is_some() {
                    self.begin_save();
                }
            }
            Err(TryRecvError::Empty) => {}
        }
    }

    /// Charge (et met en cache) la texture d'une capture.
    fn texture(
        &mut self,
        ctx: &egui::Context,
        dir: &std::path::Path,
        name: &str,
    ) -> Option<egui::TextureHandle> {
        if let Some(t) = self.textures.get(name) {
            return Some(t.clone());
        }
        let img = image::open(dir.join(name)).ok()?.to_rgba8();
        let size = [img.width() as usize, img.height() as usize];
        let color = egui::ColorImage::from_rgba_unmultiplied(size, img.as_raw());
        let handle = ctx.load_texture(name, color, egui::TextureOptions::LINEAR);
        self.textures.insert(name.to_string(), handle.clone());
        Some(handle)
    }
}

impl eframe::App for App {
    fn ui(&mut self, root: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = root.ctx().clone();
        self.poll_screen();
        self.poll_export();
        self.poll_analysis();

        // Action longue en cours (capture, sauvegarde, HTML, compression…).
        let activity = self.progress.get();

        // Tant qu'un travail tourne (enregistrement, action en cours, export),
        // les données évoluent depuis d'autres threads : rafraîchir régulièrement.
        if self.selecting_screen
            || self.is_recording()
            || activity.is_some()
            || self.export.is_some()
            || self.analysis.is_some()
        {
            ctx.request_repaint_after(Duration::from_millis(200));
        }

        egui::Panel::top("toolbar").show(root, |ui| {
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                if self.is_recording() {
                    paint_square(ui, egui::Color32::from_rgb(220, 70, 70));
                    if ui.button(self.t("Arrêter", "Stop")).clicked() {
                        self.stop();
                    }
                    ui.add_space(8.0);
                    paint_dot(ui, egui::Color32::RED);
                    ui.colored_label(egui::Color32::RED, self.t("enregistrement", "recording"));
                } else {
                    paint_dot(ui, egui::Color32::RED);
                    if ui
                        .add_enabled(
                            self.screen_ready && self.analysis.is_none() && self.export.is_none(),
                            egui::Button::new(self.t("Démarrer", "Start")),
                        )
                        .clicked()
                    {
                        self.start();
                    }
                }

                if !self.is_recording()
                    && ui
                        .add_enabled(
                            !self.selecting_screen
                                && self.analysis.is_none()
                                && self.export.is_none(),
                            egui::Button::new(self.t("Choisir la zone", "Select recording area")),
                        )
                        .clicked()
                {
                    self.select_screen();
                }

                // À droite : bouton de langue (FR/EN) puis sélecteur de thème.
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    theme_toggle(ui, self.lang);
                    let mut selected_lang = self.lang;
                    egui::ComboBox::from_id_salt("language")
                        .selected_text(match self.lang {
                            Lang::ZhCn => "简体中文",
                            Lang::En => "English",
                            Lang::Fr => "Français",
                        })
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut selected_lang, Lang::ZhCn, "简体中文");
                            ui.selectable_value(&mut selected_lang, Lang::En, "English");
                            ui.selectable_value(&mut selected_lang, Lang::Fr, "Français");
                        });
                    if selected_lang != self.lang {
                        self.set_lang(selected_lang);
                    }
                });
            });
            if !self.is_recording() {
                ui.horizontal(|ui| {
                    ui.checkbox(
                        &mut self.auto_analyze,
                        self.lang
                            .pick("Décrire après l’arrêt", "Describe steps after stopping"),
                    )
                    .on_hover_text(self.lang.pick(
                        "Envoyer les captures au MI50 configuré",
                        "Send screenshots to the configured MI50 for analysis",
                    ));
                    ui.label(self.t("Qualité des captures", "Screenshot quality"));
                    egui::ComboBox::from_id_salt("screenshot-quality")
                        .selected_text(match self.screenshot_quality as u32 {
                            100 => self.t("Sans perte", "Lossless"),
                            90 => self.t("Haute qualité", "High quality"),
                            _ => self.t("Compact", "Compact"),
                        })
                        .show_ui(ui, |ui| {
                            for (quality, label) in [
                                (100.0, self.t("Sans perte", "Lossless")),
                                (90.0, self.t("Haute qualité", "High quality")),
                                (70.0, self.t("Compact", "Compact")),
                            ] {
                                ui.selectable_value(&mut self.screenshot_quality, quality, label);
                            }
                        });
                });
            }
            if !self.is_recording() && self.analysis.is_none() && self.export.is_none() {
                ui.collapsing(self.t("Réglages vision", "Vision settings"), |ui| {
                    ui.horizontal(|ui| {
                        ui.label("API URL");
                        ui.text_edit_singleline(&mut self.vision_config.base_url);
                    });
                    ui.horizontal(|ui| {
                        ui.label("Model");
                        ui.text_edit_singleline(&mut self.vision_config.model);
                    });
                    ui.horizontal(|ui| {
                        ui.label("API Key");
                        ui.add(
                            egui::TextEdit::singleline(&mut self.vision_config.api_key)
                                .password(true),
                        );
                    });
                    ui.label(self.t(
                        "Envoyer les captures au MI50 configuré",
                        "Send screenshots to the configured MI50 for analysis",
                    ));
                    if ui
                        .button(self.t("Enregistrer les réglages", "Save settings"))
                        .clicked()
                    {
                        self.status = match self
                            .vision_config
                            .save()
                            .and_then(|()| ssr_core::vision::VisionConfig::load().map(|_| ()))
                        {
                            Ok(()) => "视觉模型设置已保存。".into(),
                            Err(error) => error,
                        };
                    }
                });
            }
            if !self.is_recording() && self.last.is_some() {
                ui.horizontal(|ui| {
                    if self.analysis.is_some() {
                        if ui
                            .button(self.t("Annuler l’analyse", "Cancel analysis"))
                            .clicked()
                        {
                            self.cancel_analysis
                                .store(true, std::sync::atomic::Ordering::Relaxed);
                            self.status = "正在取消，将保留已完成的描述…".into();
                        }
                    } else if ui
                        .add_enabled(
                            self.export.is_none(),
                            egui::Button::new(self.t("Analyser avec MI50", "Analyze with MI50")),
                        )
                        .on_hover_text(self.t(
                            "Envoyer les captures au MI50 configuré",
                            "Send screenshots to the configured MI50 for analysis",
                        ))
                        .clicked()
                    {
                        self.begin_analysis();
                    }
                    if ui
                        .add_enabled(
                            self.analysis.is_none() && self.export.is_none(),
                            egui::Button::new(self.t("Exporter à nouveau", "Save again")),
                        )
                        .clicked()
                    {
                        self.pending_save = self.last.clone();
                        self.begin_save();
                    }
                });
            }
            ui.add_space(4.0);
        });

        egui::Panel::bottom("status").show(root, |ui| {
            ui.add_space(2.0);
            ui.horizontal(|ui| {
                // Une action longue en cours prime sur le statut au repos.
                if let Some(action) = &activity {
                    ui.spinner();
                    ui.label(action);
                } else {
                    ui.label(&self.status);
                }

                // À droite : lien vers la communauté / le support (Discord).
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.hyperlink_to("💬 Discord", "https://discord.gg/T37DYHmt2j")
                        .on_hover_text(self.t("Communauté & support", "Community & support"));
                });
            });
            ui.add_space(2.0);
        });

        // Données nécessaires au rendu, extraites sous verrou puis relâchées.
        // `try_lock` : si la session est occupée (export en cours), on garde le
        // dernier instantané au lieu de bloquer l'IHM.
        if let Some(s) = self.current() {
            if let Ok(g) = s.try_lock() {
                let rows = g
                    .steps
                    .iter()
                    .map(|st| StepRow {
                        index: st.index,
                        screenshot: st.screenshot.clone(),
                        timestamp_ms: st.timestamp_ms,
                        elapsed_ms: st.elapsed_ms,
                        action: st.action.clone(),
                        window: st.window.clone(),
                        step: st.clone(),
                    })
                    .collect();
                self.snapshot = Some(Snapshot {
                    dir: g.dir.clone(),
                    rows,
                });
            }
        } else {
            self.snapshot = None;
        }
        let snapshot = self.snapshot.clone();

        // Navigation clavier dans la liste : flèches haut/bas.
        let mut nav_scroll = false;
        if let Some(rows) = snapshot.as_ref().map(|s| &s.rows).filter(|r| !r.is_empty()) {
            let cur = self
                .selected
                .and_then(|sel| rows.iter().position(|r| r.index == sel))
                .unwrap_or(rows.len() - 1);
            let (up, down) = ctx.input(|i| {
                (
                    i.key_pressed(egui::Key::ArrowUp),
                    i.key_pressed(egui::Key::ArrowDown),
                )
            });
            if down && cur + 1 < rows.len() {
                self.selected = Some(rows[cur + 1].index);
                nav_scroll = true;
            } else if up && cur > 0 {
                self.selected = Some(rows[cur - 1].index);
                nav_scroll = true;
            }
        }

        egui::Panel::left("steps")
            .resizable(true)
            .default_size(320.0)
            .show(root, |ui| {
                ui.heading(self.t("Étapes", "Steps"));
                ui.separator();
                match &snapshot {
                    Some(snap) if !snap.rows.is_empty() => {
                        egui::ScrollArea::vertical().show(ui, |ui| {
                            for row in &snap.rows {
                                let label =
                                    format!("{}. {}", row.index, row.description(self.lang));
                                let selected = self.selected == Some(row.index);
                                let resp = ui.selectable_label(selected, label);
                                if resp.clicked() {
                                    self.selected = Some(row.index);
                                }
                                // Garde la sélection visible lors de la nav clavier.
                                if selected && nav_scroll {
                                    resp.scroll_to_me(Some(egui::Align::Center));
                                }
                            }
                        });
                    }
                    _ => {
                        ui.label(self.t("Aucune étape pour l'instant.", "No steps yet."));
                    }
                }
            });

        let lang = self.lang;
        egui::CentralPanel::default().show(root, |ui| {
            let Some(snap) = snapshot else {
                ui.centered_and_justified(|ui| {
                    ui.label(self.t(
                        "Démarrez un enregistrement pour capturer les étapes.",
                        "Start a recording to capture steps.",
                    ));
                });
                return;
            };

            let selected = self.selected.or_else(|| snap.rows.last().map(|r| r.index));
            let Some(sel) = selected else { return };
            let Some(row) = snap.rows.iter().find(|r| r.index == sel) else {
                return;
            };

            ui.heading(format!(
                "{} {} — {}",
                self.t("Étape", "Step"),
                row.index,
                row.description(lang)
            ));
            step_metadata(ui, row, lang);
            if let Some(error) = &row.step.capture_error {
                ui.colored_label(egui::Color32::DARK_RED, error);
            }
            if let Some(ai) = &row.step.analysis {
                if let Some(error) = &ai.error {
                    ui.colored_label(egui::Color32::DARK_RED, error);
                }
                if !ai.visible_result.is_empty() {
                    ui.label(format!("可见结果：{}", ai.visible_result));
                }
                if !ai.uncertainty.is_empty() {
                    ui.weak(format!("待确认：{}", ai.uncertainty));
                }
                if ai.status == "complete" {
                    ui.weak(format!(
                        "{} · {}",
                        lang.pick("Description proposée par IA", "AI suggested description"),
                        ai.model
                    ));
                }
            }
            if !self.is_recording() && self.analysis.is_none() && self.export.is_none() {
                ui.collapsing(
                    lang.pick("Modifier la description", "Edit description"),
                    |ui| {
                        if self.edit_index != Some(row.index) {
                            self.edit_text = row.description(lang);
                            self.edit_index = Some(row.index);
                        }
                        ui.text_edit_multiline(&mut self.edit_text);
                        ui.horizontal(|ui| {
                            let apply = ui.button(lang.pick("Appliquer", "Apply")).clicked();
                            let clear = ui
                                .button(lang.pick("Effacer la modification", "Clear edit"))
                                .clicked();
                            if apply || clear {
                                if let Some(session) = self.current() {
                                    if let Ok(mut s) = session.lock() {
                                        if let Some(step) =
                                            s.steps.iter_mut().find(|st| st.index == row.index)
                                        {
                                            step.edited_description =
                                                if clear || self.edit_text.trim().is_empty() {
                                                    None
                                                } else {
                                                    Some(self.edit_text.clone())
                                                };
                                        }
                                    }
                                }
                            }
                        });
                    },
                );
            }
            ui.separator();
            if row.step.before_screenshot.is_some() {
                ui.checkbox(
                    &mut self.show_before,
                    lang.pick("Image avant l’action", "Before action frame"),
                );
            }
            let shown = if self.show_before {
                row.step
                    .before_screenshot
                    .as_ref()
                    .or(row.screenshot.as_ref())
            } else {
                row.screenshot.as_ref()
            };
            match shown {
                Some(name) => {
                    if let Some(tex) = self.texture(&ctx, &snap.dir, name) {
                        ui.checkbox(
                            &mut self.original_size,
                            lang.pick("Taille originale", "Original size"),
                        );
                        egui::ScrollArea::both().show(ui, |ui| {
                            let image = egui::Image::new(&tex).maintain_aspect_ratio(true);
                            if self.original_size {
                                ui.add(image.fit_to_exact_size(tex.size_vec2()));
                            } else {
                                ui.add(image.max_width(ui.available_width()));
                            }
                        });
                    } else {
                        ui.label(self.t("Capture introuvable.", "Screenshot not found."));
                    }
                }
                None => {
                    ui.label(self.t(
                        "(Étape de saisie clavier — pas de capture associée.)",
                        "(Keyboard input step — no screenshot.)",
                    ));
                }
            }
        });

        // Confirmation après annulation du choix de destination.
        if self.confirm_discard {
            let (mut retry, mut discard) = (false, false);
            egui::Window::new(self.t("Session non enregistrée", "Session not saved"))
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
                .show(&ctx, |ui| {
                    ui.label(self.t(
                        "Si vous continuez, cette session ne sera pas sauvegardée.",
                        "If you continue, this session will not be saved.",
                    ));
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        retry = ui
                            .button(self.t("Choisir un dossier", "Choose a folder"))
                            .clicked();
                        discard = ui
                            .button(self.t("Abandonner la session", "Discard session"))
                            .clicked();
                    });
                });
            if retry {
                self.confirm_discard = false;
                self.begin_save();
            } else if discard {
                self.confirm_discard = false;
                self.pending_save = None;
                self.status = self
                    .t(
                        "Session abandonnée (non enregistrée).",
                        "Session discarded (not saved).",
                    )
                    .to_string();
            }
        }
    }
}

/// Affiche le bloc de métadonnées d'une étape (date, action, fenêtre).
fn step_metadata(ui: &mut egui::Ui, row: &StepRow, lang: Lang) {
    ui.add_space(2.0);

    // Date absolue (format humain) + temps écoulé depuis le début.
    let when = human_date(row.timestamp_ms);
    let elapsed = format!("T+{:.1}s", row.elapsed_ms as f64 / 1000.0);
    ui.label(format!(
        "{} {when}   ({elapsed})",
        lang.pick("Date :", "Date:")
    ));

    // Détail de l'action.
    match &row.action {
        ssr_core::Action::Click { button, x, y, .. } => {
            let coords = (*x)
                .zip(*y)
                .map(|(x, y)| format!("   —   position ({x}, {y})"))
                .unwrap_or_default();
            ui.label(format!(
                "{} {}{coords}",
                lang.pick("Action : clic", "Action: click"),
                button.label(lang)
            ));
        }
        ssr_core::Action::TextUnconfirmed => {
            ui.label(lang.pick("Saisie non confirmée", "Unconfirmed text input"));
        }
        ssr_core::Action::Text { content } => {
            let shown = content.replace('\n', "⏎");
            ui.label(match lang {
                Lang::Fr => format!("Action : saisie « {shown} »"),
                Lang::En => format!("Action: typed “{shown}”"),
                Lang::ZhCn => format!("文字内容：“{shown}”"),
            });
        }
    }

    if let Some(source) = row.step.text_source {
        let label = match source {
            ssr_core::model::TextSource::InputMethodCommit => {
                lang.pick("Source : texte validé", "Source: input method commit")
            }
            ssr_core::model::TextSource::TextField
            | ssr_core::model::TextSource::AccessibleField => {
                lang.pick("Source : champ de texte", "Source: text field")
            }
            ssr_core::model::TextSource::KeyboardEstimate => {
                lang.pick("Source : estimation clavier", "Source: keyboard estimate")
            }
        };
        ui.weak(label);
    }
    // Fenêtre active (souvent inconnue sous Wayland).
    let title = row
        .window
        .as_ref()
        .map(|w| w.title.as_str())
        .filter(|t| !t.is_empty());
    let app = row
        .window
        .as_ref()
        .map(|w| w.app_name.as_str())
        .filter(|a| !a.is_empty());
    match (title, app) {
        (Some(t), Some(a)) => {
            ui.label(format!(
                "{} « {t} » ({a})",
                lang.pick("Fenêtre :", "Window:")
            ));
        }
        (Some(t), None) => {
            ui.label(format!("{} « {t} »", lang.pick("Fenêtre :", "Window:")));
        }
        (None, Some(a)) => {
            ui.label(format!(
                "{} {a}",
                lang.pick("Application :", "Application:")
            ));
        }
        (None, None) => {
            ui.weak(lang.pick(
                "Fenêtre : inconnue (non exposée sous Wayland)",
                "Window: unknown (not exposed under Wayland)",
            ));
        }
    }
}

/// Nom de fichier ZIP par défaut, dérivé du dossier de session.
fn default_zip_name(session: &Arc<Mutex<Session>>) -> String {
    session
        .lock()
        .ok()
        .and_then(|s| s.dir.file_name().map(|n| n.to_string_lossy().into_owned()))
        .map(|n| format!("{n}.zip"))
        .unwrap_or_else(|| "session.zip".to_string())
}

/// Formate un horodatage Unix (ms) en date locale lisible.
fn human_date(ms: u128) -> String {
    chrono::DateTime::from_timestamp_millis(ms as i64)
        .map(|dt| {
            dt.with_timezone(&chrono::Local)
                .format("%d/%m/%Y %H:%M:%S")
                .to_string()
        })
        .unwrap_or_else(|| "?".to_string())
}

/// Peint une pastille ronde pleine à la position courante (indicateur d'état).
///
/// On dessine la forme plutôt que d'utiliser un glyphe : les polices d'egui ne
/// couvrent pas les formes géométriques (`●` `■` `◐`) et afficheraient du tofu.
fn paint_dot(ui: &mut egui::Ui, color: egui::Color32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(12.0, 16.0), egui::Sense::hover());
    ui.painter().circle_filled(rect.center(), 5.0, color);
}

/// Peint un petit carré plein (idem, cf. [`paint_dot`]).
fn paint_square(ui: &mut egui::Ui, color: egui::Color32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(12.0, 16.0), egui::Sense::hover());
    let sq = egui::Rect::from_center_size(rect.center(), egui::vec2(9.0, 9.0));
    ui.painter().rect_filled(sq, 1.0, color);
}

/// Bouton de thème peint (aucun glyphe) qui cycle clair → obscur → système.
/// L'icône est un disque : vide (clair), plein (obscur), à moitié (système).
fn theme_toggle(ui: &mut egui::Ui, lang: Lang) {
    use egui::ThemePreference::{Dark, Light, System};
    let current = ui.ctx().options(|opt| opt.theme_preference);
    let tip = match current {
        Light => lang.pick(
            "Thème : clair (cliquer pour obscur)",
            "Theme: light (click for dark)",
        ),
        Dark => lang.pick(
            "Thème : obscur (cliquer pour système)",
            "Theme: dark (click for system)",
        ),
        System => lang.pick(
            "Thème : système (cliquer pour clair)",
            "Theme: system (click for light)",
        ),
    };

    let (rect, resp) = ui.allocate_exact_size(egui::vec2(24.0, 24.0), egui::Sense::click());
    let c = rect.center();
    let r = 7.0;
    let col = ui.visuals().text_color();
    let p = ui.painter();
    p.circle_stroke(c, r, egui::Stroke::new(1.5, col));
    match current {
        Light => {} // disque vide
        Dark => {
            p.circle_filled(c, r, col); // disque plein
        }
        System => {
            // Moitié droite pleine.
            use egui::epaint::PathShape;
            let pts: Vec<egui::Pos2> = (0..=16)
                .map(|i| {
                    let a = -std::f32::consts::FRAC_PI_2 + std::f32::consts::PI * (i as f32 / 16.0);
                    egui::pos2(c.x + r * a.cos(), c.y + r * a.sin())
                })
                .collect();
            p.add(PathShape::convex_polygon(pts, col, egui::Stroke::NONE));
        }
    }

    if resp.on_hover_text(tip).clicked() {
        let next = match current {
            Light => Dark,
            Dark => System,
            System => Light,
        };
        ui.ctx().set_theme(next);
    }
}

fn install_chinese_font(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert(
        "droid-cjk".into(),
        Arc::new(egui::FontData::from_static(include_bytes!(
            "../assets/DroidSansFallbackFull.ttf"
        ))),
    );
    for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
        fonts
            .families
            .entry(family)
            .or_default()
            .push("droid-cjk".into());
    }
    ctx.set_fonts(fonts);
}

impl Drop for App {
    fn drop(&mut self) {
        self.cancel_analysis
            .store(true, std::sync::atomic::Ordering::Relaxed);
    }
}
