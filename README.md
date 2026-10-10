# Support Step Recorder 中文增强版

本仓库基于 [Le-Syl21/support-step-recorder](https://github.com/Le-Syl21/support-step-recorder)，保留原项目历史与 MIT 授权声明；新增中文界面、高清截图、视觉模型步骤描述及 Linux 中文输入支持。当前版本：**v0.0.1**。

## Windows 下载与运行

在 [Releases](https://github.com/gitciu8080/support-step-recorder/releases) 下载 `ssr-v0.0.1-windows-x86_64.zip`，解压后运行 `ssr.exe`。该包由 GitHub Actions 在 Windows 环境编译，未进行数字签名。

Windows 沿用前台窗口截图；启动选区和 Fcitx5/AT-SPI 中文输入修复适用于 Linux Wayland。Windows 中文输入法记录仍需适配，不能把本版本的 Linux 中文输入修复理解为 Windows 已支持。视觉描述功能可在设置中配置兼容 OpenAI 的视觉服务，不内置服务地址或密钥。构建成功不等于已完成 Windows 实机录制验收。

源码构建：安装 Rust 和 Visual Studio C++ 构建工具后，在项目目录执行 `cargo build --release --locked -p support-step-recorder-gui`，程序位于 `target/release/ssr.exe`。

---

# Support Step Recorder (`ssr`)

> Cross-platform user-action recorder written in Rust — an open clone of Windows'
> *Problem Steps Recorder* (`psr.exe`) to document a bug or a step-by-step how-to.

**[🇬🇧 English](#-english) · [🇫🇷 Français](#-français)**

## 简体中文（KDE Wayland / MI50）

本地改造版新增中文界面与字体、默认无损 WebP、原尺寸查看、真实中文输入来源，以及 MI50 视觉步骤描述。

1. 启动软件后，在桌面共享窗口选择要录制的窗口或屏幕，然后点击“开始”。停止后再次开始会复用已选区域；需要更换时点击“选择录制区域”。取消选择后可重新选择，选择完成前不能开始录制。
2. 完成操作。中文使用 Fcitx5 最终提交文字；应用提供辅助功能或 surrounding text 时，还会记录编辑后的文本框内容（包括删除、替换和粘贴）。不支持的应用显示“内容未确认”，不会把拼音当成最终文字。
3. 勾选“停止后用 MI50 描述步骤”时，停止录制后先后台分析，再选择 ZIP 保存位置。未启用时直接保存。
4. 可以在步骤详情里修改描述，也可以点击“使用 MI50 描述步骤”重试失败的步骤，随后“重新导出”。模型描述、可见结果、待确认信息和原始事件会保留在报告中。

截图质量可选无损、高清和小体积。无损保留源图像素及尺寸；原尺寸查看不会将图片压到预览宽度。Wayland 下会尽可能保留按下前的最近帧，并在点击后约 450ms 补充新帧；新输入会取消尚未取得的后帧。时间证据不足时不声称取得完整的操作前后画面，操作后截图也不代表网络请求已完成。

### 编译与中文输入桥接

```bash
# Debian / Ubuntu；需要管理员权限时弹窗
pkexec apt-get install -y libpipewire-0.3-dev libspa-0.2-dev clang libclang-dev \
  libxdo-dev libxcb1-dev libxcb-randr0-dev libegl1-mesa-dev libgbm-dev \
  libwayland-dev libxkbcommon-dev libatspi2.0-dev libfcitx5core-dev \
  libfcitx5utils-dev cmake pkg-config
bash packaging/install-fcitx5-bridge.sh --system
qdbus org.fcitx.Fcitx5 /controller org.fcitx.Fcitx.Controller1.Restart
pkexec bash packaging/install-linux.sh
cargo run -p support-step-recorder-gui
```

桥接插件只在录制器的私有 socket 存在时发送文字，不保存输入日志；密码/敏感控件不会发送文字。AT-SPI 仍取决于目标应用是否启用并正确提供辅助功能。当前修复重点是 Linux KDE Wayland，Windows/macOS 的原按键推测路径标记为不准确来源。

### MI50 配置

界面的“视觉模型设置”可以填写服务地址、模型和密钥。配置保存在用户配置目录的 `support-step-recorder/vision.json`，Unix 文件权限为 0600，不进入项目或导出包。界面会说明截图发送到所配置的 MI50。

```json
{
  "base_url": "http://YOUR_MI50_HOST:8200/v1",
  "api_key": "YOUR_API_KEY",
  "model": "Qwen3.6-35B-A3B",
  "timeout_seconds": 120
}
```

可通过 `SSR_VISION_CONFIG` 指定配置路径，或以 `SSR_VISION_BASE_URL`、`SSR_VISION_API_KEY`、`SSR_VISION_MODEL` 覆盖文件中的值。分析可取消，已完成步骤保留；正在进行的请求最多等待配置的超时。模型不可用时可以继续导出原始记录。不要把截图中的模型建议当成已验证的操作结果。

验证：`cargo test --workspace`、`cargo clippy --workspace --all-targets -- -D warnings`；Fcitx5 实机测试为 `python3 native/fcitx5/test_bridge.py`（不要与录制同时运行）。视觉连通测试可用 `cargo run -p support-step-recorder-core --example vision_probe -- NON_SENSITIVE_SCREENSHOT.png`。

## Community & support

Questions, bug reports, beta testing, or just want to chat? Join the Discord:

[![Discord](https://img.shields.io/badge/Discord-Le--Syl21%20Tools-5865F2?logo=discord&logoColor=white)](https://discord.gg/T37DYHmt2j)

---

## 🇬🇧 English

**Website**: [le-syl21.github.io/support-step-recorder](https://le-syl21.github.io/support-step-recorder/)
(download, how-to, Linux/Wayland setup, FAQ).

On every click, `ssr` captures the relevant window, records the **mouse button**,
aggregates **typed text** between clicks, and — when available — the **active
window** info. When you stop, it asks **where to save** and writes a `.zip`
bundling the WebP screenshots, a `steps.json`, and a self-contained, replayable
**HTML report**.

The interface is available in **English and French** (auto-detected from your
locale, switchable with the `FR`/`EN` button).

### Features

- **Capture on click**: grabs the state *at* the click (like a real PSR), so a
  filled form is captured *before* it is submitted.
- **Wayland-native**: screen capture via `xdg-desktop-portal` + **PipeWire**
  (works on GNOME, KDE, wlroots…), with a one-time **consent dialog** and a
  reusable token so it isn't asked again.
- **Content-cropped** screenshots: window captures are trimmed to their content
  (no black padding), keeping files small.
- **Never blocks**: WebP encoding and disk writes happen on a background writer
  thread — clicking stays instant.
- **Replayable HTML report**: step-by-step replay mode, click any screenshot to
  **zoom** it, keyboard navigation.
- **Multi-OS by design**: Windows, macOS, Linux (X11 & Wayland). The macOS window capture
  builds in CI but has not been tried on a real Mac yet: reports welcome.

### Capture backends

Input and screen capture are abstracted behind backends, chosen at compile time
(`cfg`) and at runtime (Wayland vs. the rest):

| Component | Windows / macOS / Linux X11 | Linux Wayland |
|---|---|---|
| **Input** (clicks / keys) | [`device_query`](https://crates.io/crates/device_query) | **`evdev`** (`/dev/input`) |
| **Screen capture** | [`xcap`](https://crates.io/crates/xcap) (active window) | **`xdg-desktop-portal` + PipeWire** (user picks a window or screen) |

> **Why a dedicated Wayland path?** Wayland forbids passive global input capture
> by a normal client, so we read `/dev/input` directly (needs a `udev` rule, see
> [Linux/Wayland setup](#linuxwayland-setup)). And screen capture must go through
> the desktop portal + PipeWire — `xcap`'s Wayland path only works on wlroots
> compositors, not GNOME/KDE, so we use the portal for **all** Wayland.

### Workflow

1. **Start** — screenshots are written to a **temporary** folder (volatile).
2. Do the actions you want to document (clicks, typing).
3. **Stop** — a native **“Save as…”** dialog opens; pick a location and the
   `ZIP + HTML` report is generated there. (Cancel → confirm discard or retry.)

The `.zip` contains `report.html`, `steps.json` and all `step-XXXX.webp`. The
HTML references the WebP files (they travel together in the zip).

### Build & run

```bash
cargo run -p support-step-recorder-gui      # or: cargo run --bin ssr
cargo test --workspace    # run the tests
```

**Linux build dependencies** (for the Wayland capture path): `libpipewire-0.3-dev`,
`libspa-0.2-dev`, `clang` (bindgen). On Debian/Ubuntu:

```bash
sudo apt install libpipewire-0.3-dev libspa-0.2-dev clang
```

### Linux/Wayland setup

The `evdev` backend reads `/dev/input`. To grant access **without** adding your
user to the `input` group, install the provided `udev` rule:

```bash
sudo ./packaging/install-linux.sh
```

It installs `/etc/udev/rules.d/60-ssr-input.rules` (`TAG+="uaccess"`), granting
the active-session user read access to input devices. The `60-` prefix is
required: the `uaccess` tag must be set *before* systemd's seat rules apply.

> ⚠️ **Security**: this lets apps run by the active user read keyboard/mouse
> input — the assumed trade-off for a PSR-style recorder on Wayland. Screen
> capture additionally goes through the portal (consent popup, reusable token).

### License

MIT.

---

## 🇫🇷 Français

**Site web** : [le-syl21.github.io/support-step-recorder/fr](https://le-syl21.github.io/support-step-recorder/fr/)
(téléchargement, mode d'emploi, mise en place Linux/Wayland, FAQ).

À chaque clic, `ssr` capture la fenêtre concernée, relève le **bouton de souris**,
agrège le **texte saisi** entre deux clics et — quand c'est disponible — les
infos de la **fenêtre active**. À l'arrêt, il demande **où enregistrer** et écrit
un `.zip` regroupant les captures WebP, un `steps.json` et un **rapport HTML**
autoportant et rejouable.

L'interface est disponible en **français et anglais** (détectée depuis la locale,
basculable via le bouton `FR`/`EN`).

### Fonctionnalités

- **Capture au clic** : saisit l'état *au moment* du clic (comme un vrai PSR) —
  un formulaire rempli est donc capturé *avant* son envoi.
- **Natif Wayland** : capture d'écran via `xdg-desktop-portal` + **PipeWire**
  (GNOME, KDE, wlroots…), avec un **dialogue de consentement** unique et un jeton
  réutilisable pour ne plus le redemander.
- **Recadrage au contenu** : les captures de fenêtre sont rognées sur leur
  contenu (pas de bandes noires), fichiers plus légers.
- **Ne bloque jamais** : l'encodage WebP et l'écriture disque se font dans un
  thread d'écriture dédié — le clic reste instantané.
- **Rapport HTML rejouable** : mode relecture pas à pas, **zoom** au clic sur une
  capture, navigation clavier.
- **Multi-OS par conception** : Windows, macOS, Linux (X11 et Wayland). La capture de
  fenêtre macOS compile en CI mais n'a pas encore été essayée sur un vrai Mac : vos retours sont bienvenus.

### Backends de capture

Les entrées et la capture d'écran sont abstraites par backend, choisi à la
compilation (`cfg`) et à l'exécution (Wayland ou non) :

| Brique | Windows / macOS / Linux X11 | Linux Wayland |
|---|---|---|
| **Entrées** (clics / clavier) | [`device_query`](https://crates.io/crates/device_query) | **`evdev`** (`/dev/input`) |
| **Capture écran** | [`xcap`](https://crates.io/crates/xcap) (fenêtre active) | **`xdg-desktop-portal` + PipeWire** (l'utilisateur choisit une fenêtre ou l'écran) |

> **Pourquoi un chemin Wayland dédié ?** Wayland interdit la capture passive
> globale des entrées par un client classique : on lit donc directement
> `/dev/input` (règle `udev` requise, cf. [Mise en place](#mise-en-place-linuxwayland)).
> Et la capture d'écran passe obligatoirement par le portail + PipeWire — le
> chemin Wayland de `xcap` ne marche que sur les compositeurs wlroots, pas
> GNOME/KDE ; on utilise donc le portail pour **tout** Wayland.

### Déroulé

1. **Démarrer** — les captures sont écrites dans un dossier **temporaire**
   (volatile).
2. Réaliser les actions à documenter (clics, saisies).
3. **Arrêter** — un dialogue natif **« Enregistrer sous… »** s'ouvre ; choisis un
   emplacement et le rapport `ZIP + HTML` y est généré. (Annuler → confirmer
   l'abandon ou réessayer.)

Le `.zip` contient `report.html`, `steps.json` et toutes les `step-XXXX.webp`. Le
HTML référence les WebP (ils voyagent ensemble dans le zip).

### Compilation & lancement

```bash
cargo run -p support-step-recorder-gui      # ou : cargo run --bin ssr
cargo test --workspace    # lancer les tests
```

**Dépendances de build Linux** (chemin de capture Wayland) : `libpipewire-0.3-dev`,
`libspa-0.2-dev`, `clang` (bindgen). Sur Debian/Ubuntu :

```bash
sudo apt install libpipewire-0.3-dev libspa-0.2-dev clang
```

### Mise en place Linux/Wayland

Le backend `evdev` lit `/dev/input`. Pour y donner accès **sans** ajouter
l'utilisateur au groupe `input`, installez la règle `udev` fournie :

```bash
sudo ./packaging/install-linux.sh
```

Elle pose `/etc/udev/rules.d/60-ssr-input.rules` (`TAG+="uaccess"`), qui accorde à
l'utilisateur de la session active un accès en lecture aux périphériques
d'entrée. Le préfixe `60-` est indispensable : le tag `uaccess` doit être posé
*avant* les règles de seat de systemd.

> ⚠️ **Sécurité** : cela autorise les applications de l'utilisateur actif à lire
> les entrées clavier/souris — le compromis assumé pour un enregistreur de type
> PSR sous Wayland. La capture d'**écran** passe en plus par le portail (popup de
> consentement, jeton réutilisable).

### Licence

MIT.
