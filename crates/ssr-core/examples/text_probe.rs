//! Collect only a disposable test application's accessibility text observations.
fn main() -> Result<(), Box<dyn std::error::Error>> {
    #[cfg(target_os = "linux")]
    {
        let (tx, rx) = std::sync::mpsc::channel();
        let input = ssr_core::input::a11y::AccessibleInput::start(tx);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(8);
        let mut found = false;
        while std::time::Instant::now() < deadline {
            if let Ok(ssr_core::InputEvent::Text(event)) =
                rx.recv_timeout(std::time::Duration::from_millis(100))
            {
                if event.app.contains("SSR Text Test") {
                    println!("{}: {}", event.kind, event.text);
                    found |= event.text == "快递小助手";
                }
            }
        }
        drop(input);
        if !found {
            return Err("No confirmed Chinese field text observed".into());
        }
    }
    Ok(())
}
