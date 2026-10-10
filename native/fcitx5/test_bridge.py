"""Live bridge check using a disposable Fcitx D-Bus input context.

Requires python3-dbus and the installed/restarted addon. Never run alongside a
recording: this test owns the same socket. It restores the previous IM.
"""
import json
import os
import socket
from pathlib import Path
import dbus

root = Path(os.environ["XDG_RUNTIME_DIR"]) / "ssr"
root.mkdir(exist_ok=True)
os.chmod(root, 0o700)
path = root / "text.sock"
if path.exists():
    raise SystemExit("已有录制或遗留 socket，请先停止录制并检查，测试不会覆盖它。")
sock = socket.socket(socket.AF_UNIX, socket.SOCK_DGRAM)
sock.bind(str(path))
os.chmod(path, 0o600)
sock.settimeout(0.25)
bus = dbus.SessionBus()
controller = dbus.Interface(bus.get_object("org.fcitx.Fcitx5", "/controller"), "org.fcitx.Fcitx.Controller1")
old_im = controller.CurrentInputMethod()
frontend = dbus.Interface(bus.get_object("org.fcitx.Fcitx5", "/org/freedesktop/portal/inputmethod"), "org.fcitx.Fcitx.InputMethod1")
ic = None

def collect():
    packets = []
    try:
        while True:
            packets.append(json.loads(sock.recv(65536)))
    except socket.timeout:
        return packets

try:
    icpath, _ = frontend.CreateInputContext(dbus.Array([("program", "ssr-bridge-test")], signature="(ss)"))
    ic = dbus.Interface(bus.get_object("org.fcitx.Fcitx5", icpath), "org.fcitx.Fcitx.InputContext1")
    ic.SetCapability(dbus.UInt64(64))
    ic.FocusIn()
    ic.SetSurroundingText("", dbus.UInt32(0), dbus.UInt32(0))
    ic.SetSurroundingText("快递小助手", dbus.UInt32(5), dbus.UInt32(5))
    ic.SetSurroundingText("快递小助", dbus.UInt32(4), dbus.UInt32(4))
    ic.SetSurroundingText("粘贴中文 ABC", dbus.UInt32(8), dbus.UInt32(8))
    updates = collect()
    values = [p["text"] for p in updates if p["kind"] == "snapshot"]
    assert values == ["", "快递小助手", "快递小助", "粘贴中文 ABC"], values

    # No surrounding text: the commit watcher must still receive final Hanzi.
    ic.SetCapability(dbus.UInt64(0))
    ic.Reset()
    controller.SetCurrentIM("pinyin")
    for c in "kuaidixiaozhushou ":
        ic.ProcessKeyEvent(dbus.UInt32(ord(c)), dbus.UInt32(0), dbus.UInt32(0), False, dbus.UInt32(0))
        ic.ProcessKeyEvent(dbus.UInt32(ord(c)), dbus.UInt32(0), dbus.UInt32(0), True, dbus.UInt32(0))
    commits = [p["text"] for p in collect() if p["kind"] == "commit"]
    assert commits and any("\u4e00" <= c <= "\u9fff" for c in "".join(commits)), commits

    # Either password or sensitive flag alone must block text.
    ic.SetCapability(dbus.UInt64(64 | 8))
    ic.SetSurroundingText("秘密不可外发", dbus.UInt32(6), dbus.UInt32(6))
    private = collect()
    assert not any(p["text"] for p in private), private
    print(json.dumps({"field_updates": values, "final_commits": commits, "password_text_blocked": True}, ensure_ascii=False))
finally:
    if ic is not None:
        ic.Reset()
        controller.SetCurrentIM(old_im)
        ic.FocusOut()
        ic.DestroyIC()
    sock.close()
    path.unlink(missing_ok=True)
