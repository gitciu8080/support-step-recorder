//! Interface and report language (French, English, Simplified Chinese).
//!
//! Le cœur produit quelques chaînes destinées à l'utilisateur (descriptions
//! d'étapes, messages de progression, rapport HTML). Elles sont construites au
//! moment de l'affichage/export selon la [`Lang`] choisie, plutôt que figées.

/// Langue d'affichage.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Lang {
    /// Français.
    #[default]
    Fr,
    /// Anglais.
    En,
    ZhCn,
}

impl Lang {
    /// Prefer an explicit override or desktop LANGUAGE preference, then locale.
    /// This preserves the desktop language even when a build launcher sets LC_ALL=C.
    pub fn detect() -> Self {
        let v = std::env::var("SSR_LANG")
            .or_else(|_| std::env::var("LANGUAGE"))
            .or_else(|_| std::env::var("LC_ALL"))
            .or_else(|_| std::env::var("LC_MESSAGES"))
            .or_else(|_| std::env::var("LANG"))
            .unwrap_or_default()
            .to_lowercase();
        if v.starts_with("zh") {
            Lang::ZhCn
        } else if v.starts_with("fr") {
            Lang::Fr
        } else {
            Lang::En
        }
    }

    /// Code ISO court (`"fr"` / `"en"`), pour l'attribut `lang` du HTML.
    pub fn code(self) -> &'static str {
        match self {
            Lang::Fr => "fr",
            Lang::En => "en",
            Lang::ZhCn => "zh-CN",
        }
    }

    /// Bascule vers l'autre langue.
    pub fn toggled(self) -> Self {
        match self {
            Lang::Fr => Lang::En,
            Lang::En => Lang::ZhCn,
            Lang::ZhCn => Lang::Fr,
        }
    }

    // --- Messages de progression (barre de statut) ---------------------------

    pub fn msg_capturing(self) -> &'static str {
        self.pick("Capture de l'écran…", "Capturing screen…")
    }
    pub fn msg_saving_image(self) -> &'static str {
        self.pick("Sauvegarde de l'image…", "Saving image…")
    }
    pub fn msg_building_html(self) -> &'static str {
        self.pick("Création du HTML…", "Building HTML…")
    }
    pub fn msg_compressing(self) -> &'static str {
        self.pick("Compression…", "Compressing…")
    }

    /// Choisit la chaîne selon la langue (`fr`, `en`).
    pub fn pick(self, fr: &'static str, en: &'static str) -> &'static str {
        match self {
            Lang::Fr => fr,
            Lang::En => en,
            Lang::ZhCn => chinese(en),
        }
    }
}

/// Central catalog for the existing bilingual labels. New labels require a Chinese entry.
fn chinese(en: &'static str) -> &'static str {
    match en {
        "Choose a window or screen…" => "请选择要录制的窗口或屏幕…",
        "Selection cancelled or unavailable:" => "选择已取消或录制区域不可用：",
        "Area selected. Ready to record." => "已选择录制区域，可以开始录制。",
        "Select recording area" => "选择录制区域",
        "Ready." => "就绪。",
        "Capturing screen…" => "正在截图…",
        "Saving image…" => "正在保存截图…",
        "Building HTML…" => "正在生成报告…",
        "Compressing…" => "正在打包…",
        "Recording" => "录制中",
        "Start failed:" => "启动失败：",
        "Recording stopped —" => "录制已停止 —",
        "step(s)." => "个步骤。",
        "Save report (ZIP + HTML)" => "保存报告（ZIP + HTML）",
        "ZIP archive" => "ZIP 压缩包",
        "Session unavailable." => "无法访问会话。",
        "Generating report…" => "正在生成报告…",
        "Exported:" => "已导出：",
        "Export failed:" => "导出失败：",
        "Export interrupted." => "导出中断。",
        "Stop" => "停止",
        "Start" => "开始",
        "recording" => "正在录制",
        "Switch to French" => "切换到法语",
        "Community & support" => "社区与支持",
        "Steps" => "步骤",
        "No steps yet." => "暂时没有步骤。",
        "Start a recording to capture steps." => "点击开始，记录你的操作步骤。",
        "Step" => "步骤",
        "Screenshot not found." => "截图尚未生成或无法读取。",
        "(Keyboard input step — no screenshot.)" => "（文字输入步骤 — 没有可用截图。）",
        "Session not saved" => "会话尚未保存",
        "If you continue, this session will not be saved." => "继续操作将丢弃本次未保存的会话。",
        "Choose a folder" => "选择保存位置",
        "Discard session" => "丢弃会话",
        "Session discarded (not saved)." => "会话已丢弃（未保存）。",
        "Date:" => "时间：",
        "Action: click" => "操作：点击",
        "Window:" => "窗口：",
        "Application:" => "应用：",
        "Window: unknown (not exposed under Wayland)" => "窗口：未知（Wayland 未提供窗口信息）",
        "Theme: light (click for dark)" => "主题：浅色（点击切换深色）",
        "Theme: dark (click for system)" => "主题：深色（点击跟随系统）",
        "Theme: system (click for light)" => "主题：跟随系统（点击切换浅色）",
        "step(s) — recorded on" => "个步骤 — 录制时间",
        "Replay mode" => "回放模式",
        "Previous" => "上一步",
        "Next" => "下一步",
        "Screenshot" => "截图",
        "Enlarged screenshot" => "放大截图",
        "Unconfirmed text input" => "发生文字输入，内容未确认",
        "Text field updated" => "文本框已修改",
        "AI analysis" => "视觉分析",
        "Save again" => "重新导出",
        "Analyze with MI50" => "使用 MI50 描述步骤",
        "Cancel analysis" => "取消分析",
        "Original size" => "原尺寸",
        "Send screenshots to the configured MI50 for analysis" => {
            "将截图发送到配置的 MI50 服务进行分析"
        }
        "AI suggested description" => "模型建议描述",
        "Edit description" => "编辑步骤描述",
        "Apply" => "应用",
        "Clear edit" => "清除修改",
        "Input method text bridge" => "输入法中文桥接",
        "Source: input method commit" => "来源：输入法提交文字",
        "Source: text field" => "来源：文本框实际内容",
        "Source: keyboard estimate" => "来源：按键估计（可能不准确）",
        "No final model answer" => "模型未返回最终答复",
        "FR" => "FR",
        "EN" => "EN",
        "Describe steps after stopping" => "停止后用 MI50 描述步骤",
        "Before action frame" => "操作前画面",
        "Visible result" => "可见结果",
        "Uncertainty" => "待确认",
        "Original event" => "原始事件",
        "Screenshot quality" => "截图质量",
        "Lossless" => "无损（文字最清晰）",
        "High quality" => "高清（质量 90）",
        "Compact" => "小体积（质量 70）",
        "Vision settings" => "视觉模型设置",
        "Save settings" => "保存设置",
        _ => en,
    }
}
