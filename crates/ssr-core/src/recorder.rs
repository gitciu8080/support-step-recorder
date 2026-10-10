//! Boucle de capture : sélectionne un backend d'entrées, consomme ses
//! événements dans un thread dédié et alimente la session.

use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError, SyncSender};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use crate::capture::Screen;
use crate::i18n::Lang;
use crate::input::{self, Button, InputEvent, InputHandle, Keycode};
use crate::progress::Progress;
use crate::session::Session;
use crate::text;
use crate::writer::{Job, Writer};

/// Poignée d'enregistrement : tant qu'elle vit, les événements sont capturés.
pub struct Recorder {
    /// Session partagée avec l'IHM (lecture seule côté IHM).
    pub session: Arc<Mutex<Session>>,
    backend_name: &'static str,
    // `None` après stop()/drop : sa destruction arrête le backend et ferme le canal.
    input: Option<InputHandle>,
    #[cfg(target_os = "linux")]
    committed: Option<input::committed::CommittedInput>,
    #[cfg(target_os = "linux")]
    accessible: Option<input::a11y::AccessibleInput>,
    consumer: Option<JoinHandle<()>>,
    // File d'écriture asynchrone. Détruite APRÈS le consumer (cf. `shutdown`)
    // pour drainer les captures encore en attente d'encodage.
    writer: Option<Writer>,
}

impl Recorder {
    /// Démarre la capture dans le dossier `dir` (backend choisi automatiquement).
    ///
    /// `progress` reçoit l'action longue en cours (attente de capture, sauvegarde
    /// de l'image) pour affichage par l'IHM.
    pub fn start(dir: PathBuf, progress: Progress, lang: Lang) -> std::io::Result<Self> {
        Self::start_with_quality(dir, progress, lang, 100.0)
    }

    pub fn start_with_quality(
        dir: PathBuf,
        progress: Progress,
        lang: Lang,
        quality: f32,
    ) -> std::io::Result<Self> {
        if !quality.is_finite() || !(1.0..=100.0).contains(&quality) {
            return Err(std::io::Error::other("Invalid screenshot quality"));
        }
        Self::start_with_screen(dir, progress, lang, quality, Arc::new(Screen::new()))
    }

    /// Reuse a source selected before recording. Its owner can keep it alive
    /// between sessions, so starting again never opens another portal dialog.
    pub fn start_with_screen(
        dir: PathBuf,
        progress: Progress,
        lang: Lang,
        quality: f32,
        screen: Arc<Screen>,
    ) -> std::io::Result<Self> {
        if !quality.is_finite() || !(1.0..=100.0).contains(&quality) {
            return Err(std::io::Error::other("Invalid screenshot quality"));
        }
        let mut initial = Session::new(dir, lang)?;
        initial.screenshot_quality = quality;
        let session = Arc::new(Mutex::new(initial));
        let backend = input::select();
        let backend_name = backend.name();

        // File d'écriture asynchrone : encodage WebP + sauvegarde hors du chemin
        // du clic.
        let writer = Writer::new(progress.clone(), lang);
        let jobs = writer.sender();

        let (tx, rx) = channel::<InputEvent>();
        #[cfg(target_os = "linux")]
        let committed = input::committed::CommittedInput::start(tx.clone())?;
        #[cfg(target_os = "linux")]
        let accessible = input::a11y::AccessibleInput::start(tx.clone());
        let input = backend.start(tx)?;

        let consumer = {
            let session = session.clone();
            std::thread::Builder::new()
                .name("ssr-consumer".into())
                .spawn(move || consume(rx, session, progress, screen, jobs, lang))
                .map_err(std::io::Error::other)?
        };

        Ok(Self {
            session,
            backend_name,
            input: Some(input),
            #[cfg(target_os = "linux")]
            committed: Some(committed),
            #[cfg(target_os = "linux")]
            accessible: Some(accessible),
            consumer: Some(consumer),
            writer: Some(writer),
        })
    }

    /// Nom du backend d'entrées actif (diagnostic).
    pub fn backend_name(&self) -> &'static str {
        self.backend_name
    }

    /// Arrête la capture et renvoie la session figée.
    pub fn stop(mut self) -> Arc<Mutex<Session>> {
        // Marque l'arrêt AVANT de drainer les événements restants : ainsi le clic
        // sur « Arrêter » (encore en vol dans le canal) n'est pas capturé.
        if let Ok(mut s) = self.session.lock() {
            s.recording = false;
        }
        self.shutdown();
        if let Ok(mut s) = self.session.lock() {
            s.stop();
        }
        self.session.clone()
    }

    fn shutdown(&mut self) {
        // Détruire la poignée ferme le canal -> le consumer sort de sa boucle.
        self.input = None;
        #[cfg(target_os = "linux")]
        {
            self.committed = None;
            self.accessible = None;
        }
        if let Some(c) = self.consumer.take() {
            let _ = c.join();
        }
        // Le consumer est arrêté : plus aucun job ne sera émis. On détruit le
        // writer, ce qui draine les captures restantes puis joint son thread.
        self.writer = None;
    }
}

impl Drop for Recorder {
    fn drop(&mut self) {
        self.shutdown();
    }
}

/// Boucle du thread consommateur : applique chaque événement à la session.
fn consume(
    rx: Receiver<InputEvent>,
    session: Arc<Mutex<Session>>,
    progress: Progress,
    screen: Arc<Screen>,
    jobs: SyncSender<Job>,
    lang: Lang,
) {
    let mut last_input = Instant::now();
    let mut before: Option<(Button, crate::writer::Pixels, u128)> = None;
    let mut after_due: Option<(Instant, usize)> = None;
    let mut deferred_event = None;
    loop {
        let event = match deferred_event.take() {
            Some(event) => Ok(event),
            None => rx.recv_timeout(Duration::from_millis(100)),
        };
        match event {
            Ok(InputEvent::MouseDown {
                button,
                pos,
                timestamp_ms,
            }) => {
                after_due = None;
                before = None;
                if session.lock().map(|s| s.recording).unwrap_or(false) {
                    if let Some((pixels, _)) = screen.capture_for_click(pos) {
                        let frame_ms = pixels.timestamp_ms();
                        // Only claim a before frame if it was received BEFORE the input event.
                        if frame_ms <= timestamp_ms && timestamp_ms.saturating_sub(frame_ms) <= 500
                        {
                            before = Some((button, pixels, frame_ms));
                        }
                    }
                }
            }
            Ok(InputEvent::MouseUp { button, pos }) => {
                on_click(&session, button, pos, &progress, &screen, &jobs, lang);
                if let Ok(mut s) = session.lock() {
                    if s.recording {
                        if let Some(index) = s.steps.last().map(|step| step.index) {
                            after_due = Some((Instant::now(), index));
                            if let Some((pressed, pixels, frame_ms)) = before.take() {
                                if pressed == button {
                                    let name = s.next_screenshot_name();
                                    let path = s.dir.join(&name);
                                    if let Some(step) = s.steps.last_mut() {
                                        step.before_screenshot = Some(name);
                                        step.before_timestamp_ms = Some(frame_ms);
                                    }
                                    let quality = s.screenshot_quality;
                                    drop(s);
                                    submit_image(
                                        &session,
                                        &jobs,
                                        Job {
                                            pixels,
                                            path,
                                            quality,
                                        },
                                    );
                                }
                            }
                        }
                    }
                }
            }
            Ok(InputEvent::KeyDown(key)) => {
                after_due = None;
                on_key_down(&session, key);
                last_input = Instant::now();
            }
            Ok(InputEvent::KeyUp(key)) => on_key_up(&session, key),
            #[cfg(target_os = "linux")]
            Ok(InputEvent::Text(event)) => {
                if let Ok(mut s) = session.lock() {
                    s.observe_text(event);
                }
                last_input = Instant::now();
            }
            Err(RecvTimeoutError::Timeout) => {
                if after_due.is_some_and(|(time, _)| time.elapsed() >= Duration::from_millis(450)) {
                    if let Some((_, index)) = after_due.take() {
                        deferred_event = capture_after_click(&session, &screen, &jobs, index, &rx);
                    }
                }
                if last_input.elapsed() >= Duration::from_millis(900) {
                    capture_pending_text(&session, &screen, &jobs);
                }
            }
            Err(RecvTimeoutError::Disconnected) => break,
        }
    }
    capture_pending_text(&session, &screen, &jobs);
}

#[allow(clippy::too_many_arguments)]
fn on_click(
    session: &Arc<Mutex<Session>>,
    button: Button,
    pos: Option<(i32, i32)>,
    progress: &Progress,
    screen: &Screen,
    jobs: &SyncSender<Job>,
    lang: Lang,
) {
    // Court-circuit si l'enregistrement est déjà terminé : évite une capture
    // inutile (notamment le clic sur « Arrêter »).
    if !session.lock().map(|s| s.recording).unwrap_or(false) {
        return;
    }

    // Seule partie synchrone au clic : le snapshot de l'écran (grab xcap ou
    // dernière frame du portail). On capture l'état au moment du clic (avant que
    // l'action ne prenne effet), comme un PSR. L'encodage est délégué au writer.
    progress.set(lang.msg_capturing());
    let capture = screen.capture_for_click(pos);
    progress.clear();

    let Some((pixels, info)) = capture else {
        if let Ok(mut s) = session.lock() {
            if s.recording {
                s.record_click(button, pos, None, None);
                if let Some(step) = s.steps.last_mut() {
                    step.capture_error = Some("无法取得截图，已保留点击事件".into());
                }
            }
        }
        return;
    };

    // Enregistre l'étape (avec le nom de fichier réservé) et met la capture en
    // file d'écriture. La session reste à jour dans l'ordre ; le fichier .webp
    // apparaîtra un court instant plus tard.
    let mut s = match session.lock() {
        Ok(s) => s,
        Err(_) => return,
    };
    if !s.recording {
        return;
    }
    let frame_ms = pixels.timestamp_ms();
    let name = s.next_screenshot_name();
    let path = s.dir.join(&name);
    s.record_click(button, pos, info, Some(name.clone()));
    for step in &mut s.steps {
        if step.screenshot.as_ref() == Some(&name) {
            step.screenshot_timestamp_ms = Some(frame_ms);
        }
    }
    let quality = s.screenshot_quality;
    drop(s);
    submit_image(
        session,
        jobs,
        Job {
            pixels,
            path,
            quality,
        },
    );
}

fn on_key_down(session: &Arc<Mutex<Session>>, key: Keycode) {
    let mut s = match session.lock() {
        Ok(s) => s,
        Err(_) => return,
    };
    if !s.recording {
        return;
    }
    if text::is_shift(&key) {
        s.shift_down = true;
        return;
    }
    if matches!(
        key,
        Keycode::LControl
            | Keycode::RControl
            | Keycode::LAlt
            | Keycode::RAlt
            | Keycode::LMeta
            | Keycode::RMeta
    ) {
        s.shortcut_down = true;
        return;
    }
    if !s.shortcut_down {
        if let Some(token) = text::key_to_token(&key, s.shift_down) {
            #[cfg(target_os = "linux")]
            {
                let _ = token;
                s.note_input_activity();
            }
            #[cfg(not(target_os = "linux"))]
            s.apply_key(token);
        }
    }
}

fn on_key_up(session: &Arc<Mutex<Session>>, key: Keycode) {
    if let Ok(mut s) = session.lock() {
        if text::is_shift(&key) {
            s.shift_down = false;
        }
        if matches!(
            key,
            Keycode::LControl
                | Keycode::RControl
                | Keycode::LAlt
                | Keycode::RAlt
                | Keycode::LMeta
                | Keycode::RMeta
        ) {
            s.shortcut_down = false;
        }
    }
}

fn capture_pending_text(session: &Arc<Mutex<Session>>, screen: &Screen, jobs: &SyncSender<Job>) {
    if !session
        .lock()
        .map(|s| s.has_pending_text())
        .unwrap_or(false)
    {
        return;
    }
    let capture = screen.capture_for_click(None);
    let Ok(mut s) = session.lock() else {
        return;
    };
    if let Some((pixels, window)) = capture {
        let name = s.next_screenshot_name();
        let path = s.dir.join(&name);
        let frame_ms = pixels.timestamp_ms();
        s.flush_text_with_screenshot(window, Some(name));
        if let Some(step) = s.steps.last_mut() {
            step.screenshot_timestamp_ms = Some(frame_ms);
        }
        let quality = s.screenshot_quality;
        drop(s);
        submit_image(
            session,
            jobs,
            Job {
                pixels,
                path,
                quality,
            },
        );
    } else {
        s.flush_text(None);
    }
}

fn submit_image(session: &Arc<Mutex<Session>>, jobs: &SyncSender<Job>, job: Job) {
    // Apply bounded backpressure instead of throwing away captured pixels.
    // The input backend and UI remain on separate threads; no session lock is
    // held here, and shutdown drains the writer before exporting.
    if let Err(std::sync::mpsc::SendError(job)) = jobs.send(job) {
        if let Ok(mut s) = session.lock() {
            for step in &mut s.steps {
                if step
                    .before_screenshot
                    .as_deref()
                    .is_some_and(|name| job.path.ends_with(name))
                {
                    step.before_screenshot = None;
                    step.before_timestamp_ms = None;
                    step.capture_error = Some("截图写入线程不可用，缺少操作前画面".into());
                }
                if step
                    .screenshot
                    .as_deref()
                    .is_some_and(|name| job.path.ends_with(name))
                {
                    step.screenshot = None;
                    step.capture_error = Some("截图写入线程不可用，该步骤缺少截图".into());
                }
            }
        }
    }
}

fn capture_after_click(
    session: &Arc<Mutex<Session>>,
    screen: &Screen,
    jobs: &SyncSender<Job>,
    index: usize,
    rx: &Receiver<InputEvent>,
) -> Option<InputEvent> {
    let (pixels, _) = screen.capture_for_click(None)?;
    // If new input arrived during a native grab, do not attribute its frame to this click.
    if let Ok(event) = rx.try_recv() {
        return Some(event);
    }
    let frame_ms = pixels.timestamp_ms();
    let Ok(mut s) = session.lock() else {
        return None;
    };
    if !s.recording
        || !s
            .steps
            .last()
            .is_some_and(|step| step.index == index && frame_ms > step.timestamp_ms)
    {
        return None;
    }
    let name = s.next_screenshot_name();
    let path = s.dir.join(&name);
    let quality = s.screenshot_quality;
    if let Some(step) = s.steps.last_mut() {
        step.screenshot = Some(name);
        step.screenshot_timestamp_ms = Some(frame_ms);
    }
    drop(s);
    submit_image(
        session,
        jobs,
        Job {
            pixels,
            path,
            quality,
        },
    );
    None
}
