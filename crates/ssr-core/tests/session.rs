use std::fs;

use ssr_core::export;
use ssr_core::model::WindowInfo;
use ssr_core::session::Session;
use ssr_core::text::{key_to_token, KeyToken, Keycode};
use ssr_core::{Button, Lang};

fn unique_dir(tag: &str) -> std::path::PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("ssr-test-{tag}-{nanos}"))
}

#[test]
fn keys_translate_with_shift() {
    assert!(matches!(key_to_token(&Keycode::A, false), Some(KeyToken::Insert(s)) if s == "a"));
    assert!(matches!(key_to_token(&Keycode::A, true), Some(KeyToken::Insert(s)) if s == "A"));
    assert!(matches!(key_to_token(&Keycode::Key1, true), Some(KeyToken::Insert(s)) if s == "!"));
    assert!(matches!(
        key_to_token(&Keycode::Backspace, false),
        Some(KeyToken::Backspace)
    ));
    assert!(key_to_token(&Keycode::F5, false).is_none());
}

#[test]
fn records_text_then_click_and_exports() {
    let dir = unique_dir("flow");
    let mut session = Session::new(&dir, Lang::Fr).unwrap();

    // Saisie « Bonjour » : Maj+b puis « onjour ».
    session.apply_key(KeyToken::Insert("B".into()));
    for c in "onjour".chars() {
        session.apply_key(KeyToken::Insert(c.to_string()));
    }

    let window = Some(WindowInfo {
        title: "Éditeur".into(),
        app_name: "demo".into(),
        pid: 42,
        x: 100,
        y: 50,
        width: 800,
        height: 600,
    });

    // Un clic vide d'abord le texte (étape Text), puis crée l'étape Click.
    session.record_click(Button::Left, Some((260, 130)), window, None);
    session.stop();

    assert_eq!(session.steps.len(), 2, "une étape Text + une étape Click");
    assert!(matches!(
        session.steps[0].action,
        ssr_core::Action::Text { .. }
    ));
    if let ssr_core::Action::Click { rel_x, rel_y, .. } = session.steps[1].action {
        assert_eq!(
            (rel_x, rel_y),
            (Some(160), Some(80)),
            "coords relatives à la fenêtre"
        );
    } else {
        panic!("la 2e étape doit être un clic");
    }

    // Export complet.
    let zip = dir.with_extension("zip");
    export::export_zip(&session, &zip, &ssr_core::Progress::new(), Lang::Fr).unwrap();
    assert!(zip.exists(), "l'archive ZIP doit être créée");
    assert!(dir.join("report.html").exists());
    assert!(dir.join("steps.json").exists());

    let html = fs::read_to_string(dir.join("report.html")).unwrap();
    assert!(html.contains("Bonjour"));
    assert!(html.contains("Support Step Recorder"));

    let _ = fs::remove_dir_all(&dir);
    let _ = fs::remove_file(&zip);
}

#[cfg(target_os = "linux")]
fn observe(session: &mut Session, kind: &str, context: &str, text: &str) {
    session.observe_text(ssr_core::input::committed::TextObservation {
        kind: kind.into(),
        context: context.into(),
        app: "中文测试应用".into(),
        text: text.into(),
    });
}

#[test]
#[cfg(target_os = "linux")]
fn final_chinese_commits_survive_stop_and_export_without_pinyin() {
    let dir = unique_dir("chinese");
    let mut session = Session::new(&dir, Lang::ZhCn).unwrap();
    observe(&mut session, "focus", "field-1", "");
    session.note_input_activity();
    observe(&mut session, "commit", "field-1", "快递");
    session.note_input_activity();
    observe(&mut session, "commit", "field-1", "小助手");
    session.stop();
    assert_eq!(session.steps.len(), 1);
    assert!(
        matches!(&session.steps[0].action, ssr_core::Action::Text { content } if content == "快递小助手")
    );
    assert!(session.steps[0]
        .display_description(Lang::ZhCn)
        .contains("快递小助手"));
    export::write_report(&session, Lang::ZhCn).unwrap();
    for name in ["report.html", "steps.json"] {
        let data = fs::read_to_string(dir.join(name)).unwrap();
        assert!(data.contains("快递小助手"));
        assert!(!data.contains("kuaidi"));
    }
    assert!(fs::read_to_string(dir.join("report.html"))
        .unwrap()
        .contains("lang=\"zh-CN\""));
    fs::remove_dir_all(dir).unwrap();
}

#[test]
#[cfg(target_os = "linux")]
fn field_updates_override_commits_and_preserve_deletions_and_paste() {
    let dir = unique_dir("field");
    let mut session = Session::new(&dir, Lang::ZhCn).unwrap();
    observe(&mut session, "snapshot", "field-1", "原有内容");
    observe(&mut session, "commit", "field-1", "快递小助手");
    observe(&mut session, "snapshot", "field-1", "快递小助手");
    observe(&mut session, "snapshot", "field-1", "快递小助");
    session.flush_text(None);
    observe(&mut session, "snapshot", "field-1", "");
    session.flush_text(None);
    observe(&mut session, "snapshot", "field-1", "粘贴中文 ABC");
    session.stop();
    let values: Vec<_> = session
        .steps
        .iter()
        .map(|s| match &s.action {
            ssr_core::Action::Text { content } => content.as_str(),
            _ => panic!("confirmed field text expected"),
        })
        .collect();
    assert_eq!(values, ["快递小助", "", "粘贴中文 ABC"]);
    fs::remove_dir_all(dir).unwrap();
}

#[test]
#[cfg(target_os = "linux")]
fn sensitive_fields_discard_pending_text_and_unknown_input_is_honest() {
    let dir = unique_dir("privacy");
    let mut session = Session::new(&dir, Lang::ZhCn).unwrap();
    observe(&mut session, "commit", "field-1", "不应记录");
    observe(&mut session, "sensitive", "field-1", "");
    session.flush_text(None);
    assert!(session.steps.is_empty());
    session.note_input_activity();
    session.stop();
    assert!(matches!(
        session.steps[0].action,
        ssr_core::Action::TextUnconfirmed
    ));
    fs::remove_dir_all(dir).unwrap();
}

#[test]
#[cfg(target_os = "linux")]
fn accessible_field_edits_are_not_recorded_as_original_field_contents() {
    let dir = unique_dir("a11y");
    let mut session = Session::new(&dir, Lang::ZhCn).unwrap();
    observe(&mut session, "a11y_focus", "a11y:1", "原有");
    assert!(!session.has_pending_text());
    observe(&mut session, "a11y_snapshot", "a11y:1", "快递小助手");
    session.stop();
    assert_eq!(session.steps.len(), 1);
    assert_eq!(
        session.steps[0].text_source,
        Some(ssr_core::model::TextSource::AccessibleField)
    );
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn lossless_screenshot_round_trip_preserves_pixels() {
    let image = image::RgbaImage::from_fn(64, 48, |x, y| {
        image::Rgba([(x * 3) as u8, (y * 5) as u8, ((x + y) * 2) as u8, 255])
    });
    let encoded = ssr_core::capture::encode_webp(&image, 100.0);
    let decoded = image::load_from_memory(&encoded).unwrap().to_rgba8();
    assert_eq!(image, decoded);
}

#[test]
fn shared_screenshot_is_archived_once_and_ai_edits_reach_report() {
    let dir = unique_dir("shared");
    let mut session = Session::new(&dir, Lang::ZhCn).unwrap();
    session.apply_key(KeyToken::Insert("快递小助手".into()));
    fs::write(
        dir.join("step-0001.webp"),
        ssr_core::capture::encode_webp(&image::RgbaImage::new(4, 4), 100.0),
    )
    .unwrap();
    session.record_click(Button::Left, None, None, Some("step-0001.webp".into()));
    session.stop();
    session.steps[0].analysis = Some(ssr_core::model::VisualAnalysis {
        status: "complete".into(),
        description: "在搜索框输入快递小助手".into(),
        visible_result: "搜索框显示中文".into(),
        uncertainty: "未确认是否提交搜索".into(),
        error: None,
        model: "test".into(),
    });
    session.steps[1].edited_description = Some("点击搜索按钮 <测试>".into());
    let zip = dir.with_extension("zip");
    export::export_zip(&session, &zip, &ssr_core::Progress::new(), Lang::ZhCn).unwrap();
    let archive = zip::ZipArchive::new(fs::File::open(&zip).unwrap()).unwrap();
    assert_eq!(archive.len(), 3);
    let html = fs::read_to_string(dir.join("report.html")).unwrap();
    assert!(html.contains("在搜索框输入快递小助手"));
    assert!(html.contains("点击搜索按钮 &lt;测试&gt;"));
    let exported: Vec<ssr_core::Step> =
        serde_json::from_str(&fs::read_to_string(dir.join("steps.json")).unwrap()).unwrap();
    assert_eq!(exported[0].description, "在搜索框输入快递小助手");
    assert_eq!(exported[1].description, "点击搜索按钮 <测试>");
    let old = r#"{"index":1,"timestamp_ms":1,"elapsed_ms":1,"action":{"type":"text","content":"中文"},"description":"old"}"#;
    let restored: ssr_core::Step = serde_json::from_str(old).unwrap();
    assert!(restored.analysis.is_none());
    fs::remove_file(zip).unwrap();
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn missing_evidence_does_not_overwrite_a_previous_export() {
    let dir = unique_dir("missing-evidence");
    let mut session = Session::new(&dir, Lang::ZhCn).unwrap();
    session.record_click(Button::Left, None, None, Some("missing.webp".into()));
    session.stop();
    let zip = dir.with_extension("zip");
    fs::write(&zip, "previous valid report").unwrap();
    assert!(export::export_zip(&session, &zip, &ssr_core::Progress::new(), Lang::ZhCn).is_err());
    assert_eq!(fs::read_to_string(&zip).unwrap(), "previous valid report");
    fs::remove_file(zip).unwrap();
    fs::remove_dir_all(dir).unwrap();
}
