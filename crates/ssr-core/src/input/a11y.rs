use super::{committed::TextObservation, InputEvent};
use std::ffi::{c_char, c_int, c_void, CStr};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc::Sender,
    Arc,
};
use std::thread::JoinHandle;

type TextCallback =
    extern "C" fn(*mut c_void, *const c_char, *const c_char, *const c_char, *const c_char);
unsafe extern "C" {
    fn ssr_a11y_run(
        text: TextCallback,
        stop: extern "C" fn(*mut c_void) -> c_int,
        data: *mut c_void,
    ) -> c_int;
}
struct State {
    tx: Sender<InputEvent>,
    stop: Arc<AtomicBool>,
}
extern "C" fn on_text(
    data: *mut c_void,
    kind: *const c_char,
    context: *const c_char,
    app: *const c_char,
    text: *const c_char,
) {
    // C owns these strings for the callback; copy before returning.
    unsafe {
        let state = &*(data as *const State);
        let copy = |p| CStr::from_ptr(p).to_string_lossy().into_owned();
        let _ = state.tx.send(InputEvent::Text(TextObservation {
            kind: copy(kind),
            context: copy(context),
            app: copy(app),
            text: copy(text),
        }));
    }
}
extern "C" fn stopped(data: *mut c_void) -> c_int {
    unsafe { (&*(data as *const State)).stop.load(Ordering::Relaxed) as c_int }
}
pub struct AccessibleInput {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}
impl AccessibleInput {
    pub fn start(tx: Sender<InputEvent>) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let flag = stop.clone();
        let thread = std::thread::spawn(move || {
            let mut state = State { tx, stop: flag };
            unsafe {
                ssr_a11y_run(on_text, stopped, &mut state as *mut State as *mut c_void);
            }
        });
        Self {
            stop,
            thread: Some(thread),
        }
    }
}
impl Drop for AccessibleInput {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}
