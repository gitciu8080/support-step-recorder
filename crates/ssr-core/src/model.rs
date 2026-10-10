//! Modèle de données d'une session de capture.

use serde::{Deserialize, Serialize};

use crate::i18n::Lang;
use crate::input::Button;

/// Informations sur la fenêtre active au moment d'une action.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WindowInfo {
    pub title: String,
    pub app_name: String,
    pub pid: u32,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

/// Nature d'une étape enregistrée.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Action {
    /// Clic souris. Coordonnées globales et relatives à la fenêtre — `None`
    /// quand la plateforme ne les fournit pas (Wayland/evdev).
    Click {
        button: Button,
        #[serde(skip_serializing_if = "Option::is_none")]
        x: Option<i32>,
        #[serde(skip_serializing_if = "Option::is_none")]
        y: Option<i32>,
        #[serde(skip_serializing_if = "Option::is_none")]
        rel_x: Option<i32>,
        #[serde(skip_serializing_if = "Option::is_none")]
        rel_y: Option<i32>,
    },
    /// Texte saisi au clavier (agrégé entre deux clics).
    Text { content: String },
    /// A physical typing activity without verified committed text.
    TextUnconfirmed,
}

/// Une étape de la session : une action, son contexte et sa capture éventuelle.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Step {
    pub index: usize,
    /// Horodatage absolu (ms depuis l'epoch Unix).
    pub timestamp_ms: u128,
    /// Temps écoulé depuis le début de la session (ms).
    pub elapsed_ms: u128,
    pub action: Action,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub window: Option<WindowInfo>,
    /// Nom du fichier WebP de la capture, relatif au dossier de session.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub screenshot: Option<String>,
    /// Description lisible de l'étape, façon MS PSR (dans la langue de la session).
    pub description: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text_source: Option<TextSource>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub analysis: Option<VisualAnalysis>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub edited_description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capture_error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub before_screenshot: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub before_timestamp_ms: Option<u128>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub screenshot_timestamp_ms: Option<u128>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TextSource {
    KeyboardEstimate,
    InputMethodCommit,
    TextField,
    AccessibleField,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VisualAnalysis {
    pub status: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub visible_result: String,
    #[serde(default)]
    pub uncertainty: String,
    #[serde(default)]
    pub error: Option<String>,
    #[serde(default)]
    pub model: String,
}

impl Step {
    /// One description selection path for GUI and report.
    pub fn display_description(&self, lang: Lang) -> String {
        if let Some(edit) = &self.edited_description {
            return edit.clone();
        }
        if let Some(ai) = &self.analysis {
            if ai.status == "complete" && !ai.description.trim().is_empty() {
                return ai.description.clone();
            }
        }
        if matches!(
            self.text_source,
            Some(TextSource::TextField | TextSource::AccessibleField)
        ) {
            if let Action::Text { content } = &self.action {
                return format!(
                    "{}：“{}”",
                    lang.pick("Champ modifié", "Text field updated"),
                    content.replace('\n', "⏎")
                );
            }
        }
        Self::build_description(&self.action, self.window.as_ref(), lang)
    }

    /// Construit la description lisible à partir de l'action et du contexte,
    /// dans la langue donnée.
    pub fn build_description(action: &Action, window: Option<&WindowInfo>, lang: Lang) -> String {
        let target = window
            .map(|w| {
                if w.app_name.is_empty() {
                    format!("« {} »", w.title)
                } else {
                    format!("« {} » ({})", w.title, w.app_name)
                }
            })
            .unwrap_or_else(|| match lang {
                Lang::Fr => "la fenêtre active".to_string(),
                Lang::En => "the active window".to_string(),
                Lang::ZhCn => "当前窗口".to_string(),
            });

        match action {
            Action::TextUnconfirmed => lang
                .pick("Saisie non confirmée", "Unconfirmed text input")
                .to_string(),
            Action::Click { button, .. } => {
                let b = button.label(lang);
                match lang {
                    Lang::Fr => format!("Clic {b} sur {target}"),
                    Lang::En => format!("{} click on {target}", capitalize(&b)),
                    Lang::ZhCn => format!("在{target}点击{b}"),
                }
            }
            Action::Text { content } => {
                let shown = content.replace('\n', "⏎");
                match lang {
                    Lang::Fr => format!("Saisie « {shown} » dans {target}"),
                    Lang::En => format!("Typed “{shown}” in {target}"),
                    Lang::ZhCn => format!("在{target}输入“{shown}”"),
                }
            }
        }
    }
}

/// Met la première lettre en majuscule (« left » → « Left »).
fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}
