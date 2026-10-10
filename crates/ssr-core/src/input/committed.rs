//! Fcitx5 bridge: final UTF-8 commits and surrounding field updates, never key guesses.
use super::InputEvent;
use std::io;
use std::os::unix::{fs::PermissionsExt, net::UnixDatagram};
use std::path::PathBuf;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc::Sender,
    Arc,
};
use std::thread::JoinHandle;
use std::time::Duration;

#[derive(Debug, Clone, serde::Deserialize)]
pub struct TextObservation {
    pub kind: String,
    pub context: String,
    pub app: String,
    pub text: String,
}

pub struct CommittedInput {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
    path: PathBuf,
}

impl CommittedInput {
    pub fn start(tx: Sender<InputEvent>) -> io::Result<Self> {
        let runtime = std::env::var_os("XDG_RUNTIME_DIR")
            .ok_or_else(|| io::Error::other("XDG_RUNTIME_DIR unavailable"))?;
        let dir = PathBuf::from(runtime).join("ssr");
        if let Ok(meta) = std::fs::symlink_metadata(&dir) {
            if !meta.is_dir() || meta.file_type().is_symlink() {
                return Err(io::Error::other("Invalid text bridge directory"));
            }
        } else {
            std::fs::create_dir(&dir)?;
        }
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700))?;
        let path = dir.join("text.sock");
        // Never unlink another live recorder's socket.
        if path.exists() {
            let probe = UnixDatagram::unbound()?;
            if probe.connect(&path).is_ok() {
                return Err(io::Error::other("Another text recorder is active"));
            }
            std::fs::remove_file(&path)?;
        }
        let socket = UnixDatagram::bind(&path)?;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
        socket.set_read_timeout(Some(Duration::from_millis(80)))?;
        let stop = Arc::new(AtomicBool::new(false));
        let flag = stop.clone();
        let thread = std::thread::spawn(move || {
            let mut buffer = vec![0u8; 65536];
            while !flag.load(Ordering::Relaxed) {
                if let Ok(n) = socket.recv(&mut buffer) {
                    if let Ok(event) = serde_json::from_slice::<TextObservation>(&buffer[..n]) {
                        if tx.send(InputEvent::Text(event)).is_err() {
                            break;
                        }
                    }
                }
            }
            // Preserve final commits already queued when Stop is pressed.
            let _ = socket.set_nonblocking(true);
            for _ in 0..256 {
                match socket.recv(&mut buffer) {
                    Ok(n) => {
                        if let Ok(event) = serde_json::from_slice::<TextObservation>(&buffer[..n]) {
                            let _ = tx.send(InputEvent::Text(event));
                        }
                    }
                    Err(_) => break,
                }
            }
        });
        Ok(Self {
            stop,
            thread: Some(thread),
            path,
        })
    }
}

impl Drop for CommittedInput {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
        let _ = std::fs::remove_file(&self.path);
    }
}
