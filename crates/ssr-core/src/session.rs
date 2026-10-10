//! Session de capture : accumule les étapes et gère le tampon de texte.

use std::io;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use chrono::{DateTime, Local};

use crate::model::{Action, Step, TextSource, WindowInfo};
use crate::text::KeyToken;

/// Une session d'enregistrement vivante.
pub struct Session {
    /// Dossier où sont écrites les captures et le rapport.
    pub dir: PathBuf,
    /// Date de début (pour le rapport).
    pub started_at: DateTime<Local>,
    /// Étapes enregistrées dans l'ordre chronologique.
    pub steps: Vec<Step>,
    /// `true` tant que la capture est active.
    pub recording: bool,
    /// État de la touche Maj (suivi pour la casse du texte saisi).
    pub shift_down: bool,
    /// Langue dans laquelle les descriptions d'étapes sont construites.
    pub lang: crate::i18n::Lang,

    start_ms: u128,
    pending_text: String,
    screenshot_counter: usize,
    pending_source: Option<TextSource>,
    pending_input: bool,
    pending_field_change: bool,
    text_window: Option<WindowInfo>,
    #[cfg(target_os = "linux")]
    text_context: String,
    #[cfg(target_os = "linux")]
    field_value: Option<String>,
    pub shortcut_down: bool,
    pub screenshot_quality: f32,
    #[cfg(target_os = "linux")]
    a11y_context: String,
    #[cfg(target_os = "linux")]
    a11y_value: Option<String>,
}

impl Session {
    /// Crée une session et son dossier de sortie.
    pub fn new(dir: impl AsRef<Path>, lang: crate::i18n::Lang) -> io::Result<Self> {
        let dir = dir.as_ref().to_path_buf();
        std::fs::create_dir_all(&dir)?;
        Ok(Self {
            dir,
            started_at: Local::now(),
            steps: Vec::new(),
            recording: true,
            shift_down: false,
            lang,
            start_ms: now_ms(),
            pending_text: String::new(),
            screenshot_counter: 0,
            pending_source: None,
            pending_input: false,
            pending_field_change: false,
            text_window: None,
            #[cfg(target_os = "linux")]
            text_context: String::new(),
            #[cfg(target_os = "linux")]
            field_value: None,
            shortcut_down: false,
            screenshot_quality: 100.0,
            #[cfg(target_os = "linux")]
            a11y_context: String::new(),
            #[cfg(target_os = "linux")]
            a11y_value: None,
        })
    }

    /// Applique un jeton clavier au tampon de texte courant.
    pub fn apply_key(&mut self, token: KeyToken) {
        self.pending_source = Some(TextSource::KeyboardEstimate);
        match token {
            KeyToken::Insert(s) => self.pending_text.push_str(&s),
            KeyToken::Backspace => {
                self.pending_text.pop();
            }
        }
    }

    /// Vide le tampon de texte en une étape `Text`, s'il n'est pas vide.
    pub fn has_pending_text(&self) -> bool {
        self.pending_input || !self.pending_text.trim().is_empty() || self.pending_field_change
    }

    pub fn note_input_activity(&mut self) {
        self.pending_input = true;
    }

    #[cfg(target_os = "linux")]
    pub fn observe_text(&mut self, event: crate::input::committed::TextObservation) {
        if event.kind.starts_with("a11y_") {
            if self.a11y_context != event.context {
                self.flush_text(None);
                self.a11y_context = event.context;
                self.a11y_value = None;
            }
            if self
                .a11y_value
                .as_ref()
                .is_some_and(|old| old != &event.text)
            {
                self.pending_text = event.text.clone();
                self.pending_source = Some(TextSource::AccessibleField);
                self.pending_field_change = true;
                self.pending_input = false;
                self.text_window = Some(WindowInfo {
                    app_name: event.app,
                    ..Default::default()
                });
            }
            self.a11y_value = Some(event.text);
            return;
        }
        if self.text_context != event.context {
            self.flush_text(None);
            self.text_context = event.context.clone();
            self.field_value = None;
        }
        if event.kind == "sensitive" {
            self.pending_text.clear();
            self.pending_input = false;
            self.pending_field_change = false;
            self.pending_source = None;
            self.field_value = None;
            return;
        }
        self.text_window = Some(WindowInfo {
            app_name: event.app,
            ..Default::default()
        });
        match event.kind.as_str() {
            "focus" => {}
            "commit" => {
                if self.pending_source == Some(TextSource::TextField) {
                    // A later field update will supersede this fallback commit.
                    self.pending_text.clear();
                }
                self.pending_text.push_str(&event.text);
                self.pending_source = Some(TextSource::InputMethodCommit);
                self.pending_input = false;
            }
            "snapshot" => {
                if self
                    .field_value
                    .as_ref()
                    .is_some_and(|old| old != &event.text)
                {
                    // This is the actual edited field value, not a guess of inserted characters.
                    self.pending_text = event.text.clone();
                    self.pending_source = Some(TextSource::TextField);
                    self.pending_field_change = true;
                    self.pending_input = false;
                }
                self.field_value = Some(event.text);
            }
            _ => {}
        }
    }

    pub fn flush_text(&mut self, window: Option<WindowInfo>) {
        self.flush_text_with_screenshot(window, None);
    }

    pub fn flush_text_with_screenshot(
        &mut self,
        window: Option<WindowInfo>,
        screenshot: Option<String>,
    ) {
        let content = std::mem::take(&mut self.pending_text);
        let source = self.pending_source.take();
        let activity = std::mem::take(&mut self.pending_input);
        let changed = std::mem::take(&mut self.pending_field_change);
        let action = if !content.trim().is_empty() || changed {
            Action::Text { content }
        } else if activity {
            Action::TextUnconfirmed
        } else {
            return;
        };
        let window = self.text_window.clone().or(window);
        self.push_step(action, window, screenshot);
        if let Some(step) = self.steps.last_mut() {
            step.text_source = source;
        }
    }

    /// Enregistre un clic (le texte en attente est d'abord vidé).
    pub fn record_click(
        &mut self,
        button: crate::input::Button,
        global: Option<(i32, i32)>,
        window: Option<WindowInfo>,
        screenshot: Option<String>,
    ) {
        // Le texte tapé avant ce clic constitue une étape distincte qui le précède.
        self.flush_text_with_screenshot(window.clone(), screenshot.clone());

        // Position connue sur X11/Windows/macOS ; absente sur Wayland (evdev).
        let pos = global.map(|(x, y)| {
            let (rx, ry) = match &window {
                Some(w) => (x - w.x, y - w.y),
                None => (x, y),
            };
            (x, y, rx, ry)
        });
        let action = match pos {
            Some((x, y, rel_x, rel_y)) => Action::Click {
                button,
                x: Some(x),
                y: Some(y),
                rel_x: Some(rel_x),
                rel_y: Some(rel_y),
            },
            None => Action::Click {
                button,
                x: None,
                y: None,
                rel_x: None,
                rel_y: None,
            },
        };
        self.push_step(action, window, screenshot);
    }

    /// Réserve le prochain nom de fichier de capture.
    pub fn next_screenshot_name(&mut self) -> String {
        self.screenshot_counter += 1;
        format!("step-{:04}.webp", self.screenshot_counter)
    }

    /// Stoppe la session et vide le texte restant.
    pub fn stop(&mut self) {
        self.flush_text(None);
        self.recording = false;
    }

    fn push_step(
        &mut self,
        action: Action,
        window: Option<WindowInfo>,
        screenshot: Option<String>,
    ) {
        let now = now_ms();
        let description = Step::build_description(&action, window.as_ref(), self.lang);
        self.steps.push(Step {
            index: self.steps.len() + 1,
            timestamp_ms: now,
            elapsed_ms: now.saturating_sub(self.start_ms),
            action,
            window,
            screenshot,
            description,
            text_source: None,
            analysis: None,
            edited_description: None,
            capture_error: None,
            before_screenshot: None,
            before_timestamp_ms: None,
            screenshot_timestamp_ms: None,
        });
    }
}

pub(crate) fn now_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
}
