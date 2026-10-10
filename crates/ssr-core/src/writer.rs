//! Écriture asynchrone des captures.
//!
//! Au clic, on ne fait que le strict nécessaire (snapshot de l'image brute +
//! métadonnées), poussé dans une file. Un thread dédié se charge du travail
//! coûteux — conversion en RGBA, encodage WebP, écriture disque — sans jamais
//! bloquer l'IHM. Si l'encodage prend du retard, le consommateur attend une
//! place dans la file bornée plutôt que de supprimer une capture.

use std::path::PathBuf;
use std::sync::mpsc::{sync_channel, Receiver, SyncSender};
use std::thread::JoinHandle;

use image::RgbaImage;

use crate::capture::encode_webp;
use crate::progress::Progress;

/// Pixels d'une capture, prêts (ou presque) à encoder.
pub enum Pixels {
    /// Déjà en RGBA (chemin `xcap`).
    Rgba(RgbaImage),
    /// Frame brute du portail (BGRx…), convertie côté writer (chemin Wayland).
    #[cfg(target_os = "linux")]
    Portal(crate::screencast::Frame),
}

impl Pixels {
    pub fn timestamp_ms(&self) -> u128 {
        match self {
            #[cfg(target_os = "linux")]
            Pixels::Portal(frame) => frame.timestamp_ms,
            Pixels::Rgba(_) => crate::session::now_ms(),
        }
    }
}

/// Un travail d'écriture : une capture à encoder et déposer sur disque.
pub struct Job {
    pub pixels: Pixels,
    pub path: PathBuf,
    pub quality: f32,
}

/// File d'écriture asynchrone. Tant qu'elle vit, un thread consomme les travaux ;
/// à la destruction, la file est fermée puis vidée avant de rendre la main.
pub struct Writer {
    tx: Option<SyncSender<Job>>,
    thread: Option<JoinHandle<()>>,
}

impl Writer {
    /// Démarre le thread d'écriture. `progress` affiche l'activité en cours.
    pub fn new(progress: Progress, lang: crate::i18n::Lang) -> Self {
        let (tx, rx) = sync_channel::<Job>(4);
        let thread = std::thread::Builder::new()
            .name("ssr-writer".into())
            .spawn(move || run(rx, progress, lang))
            .ok();
        Self {
            tx: Some(tx),
            thread,
        }
    }

    /// Émetteur pour déposer des travaux depuis le thread de capture.
    pub fn sender(&self) -> SyncSender<Job> {
        self.tx.clone().expect("writer actif")
    }
}

impl Drop for Writer {
    fn drop(&mut self) {
        // Fermer le canal fait sortir le thread une fois la file vidée.
        self.tx = None;
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

/// Boucle du thread : encode et écrit chaque capture, dans l'ordre d'arrivée.
fn run(rx: Receiver<Job>, progress: Progress, lang: crate::i18n::Lang) {
    while let Ok(job) = rx.recv() {
        progress.set(lang.msg_saving_image());
        if let Err(err) = write_job(job) {
            eprintln!("ssr: écriture d'une capture échouée : {err}");
        }
        // On efface seulement si plus rien n'attend, pour ne pas clignoter.
        // (best-effort : une nouvelle capture réaffichera le message).
        progress.clear();
    }
}

fn write_job(job: Job) -> std::io::Result<()> {
    #[cfg(not(target_os = "linux"))]
    let Pixels::Rgba(image) = job.pixels;
    #[cfg(target_os = "linux")]
    let image = match job.pixels {
        Pixels::Rgba(img) => img,
        #[cfg(target_os = "linux")]
        Pixels::Portal(frame) => {
            // Recadre sur le contenu (retire le noir dont Mutter entoure les
            // captures de fenêtre).
            let (w, h) = (frame.width, frame.height);
            let rgba = frame
                .to_rgba()
                .ok_or_else(|| std::io::Error::other("Unsupported frame format"))?;
            RgbaImage::from_raw(w, h, rgba)
                .ok_or_else(|| std::io::Error::other("dimensions de frame incohérentes"))?
        }
    };
    let bytes = encode_webp(&image, job.quality);
    std::fs::write(&job.path, bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn burst_larger_than_queue_is_fully_written_before_shutdown_returns() {
        let dir = std::env::temp_dir().join(format!(
            "ssr-writer-burst-{}-{}",
            std::process::id(),
            crate::session::now_ms()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let writer = Writer::new(Progress::new(), crate::i18n::Lang::ZhCn);
        let sender = writer.sender();
        for index in 0..32u8 {
            let image = RgbaImage::from_pixel(128, 128, image::Rgba([index, 37, 211, 255]));
            assert!(sender
                .send(Job {
                    pixels: Pixels::Rgba(image),
                    path: dir.join(format!("{index}.webp")),
                    quality: 100.0
                })
                .is_ok());
        }
        drop(sender);
        drop(writer);
        for index in 0..32u8 {
            let image = image::open(dir.join(format!("{index}.webp")))
                .unwrap()
                .to_rgba8();
            assert_eq!(image.get_pixel(64, 64).0, [index, 37, 211, 255]);
        }
        std::fs::remove_dir_all(dir).unwrap();
    }
}
