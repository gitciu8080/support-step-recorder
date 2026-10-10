//! Manual MI50 smoke test with an explicitly supplied non-sensitive screenshot.
use ssr_core::{
    model::Action,
    vision::{describe_step, VisionConfig},
    Lang, Step,
};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let source = std::env::args()
        .nth(1)
        .ok_or("Usage: vision_probe screenshot.png")?;
    let dir = std::env::temp_dir().join(format!("ssr-vision-probe-{}", std::process::id()));
    std::fs::create_dir_all(&dir)?;
    let image = image::open(source)?.to_rgba8();
    std::fs::write(
        dir.join("probe.webp"),
        ssr_core::capture::encode_webp(&image, 100.0),
    )?;
    let action = Action::Text {
        content: "快递小助手".into(),
    };
    let step = Step {
        index: 1,
        timestamp_ms: 0,
        elapsed_ms: 0,
        description: Step::build_description(&action, None, Lang::ZhCn),
        action,
        window: None,
        screenshot: Some("probe.webp".into()),
        text_source: Some(ssr_core::model::TextSource::InputMethodCommit),
        analysis: None,
        edited_description: None,
        capture_error: None,
        before_screenshot: None,
        before_timestamp_ms: None,
        screenshot_timestamp_ms: None,
    };
    let result = describe_step(&VisionConfig::load()?, &step, &dir, None);
    std::fs::remove_dir_all(&dir)?;
    println!("{}", serde_json::to_string_pretty(&result?)?);
    Ok(())
}
