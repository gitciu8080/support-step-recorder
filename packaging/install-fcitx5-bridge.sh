#!/usr/bin/env bash
set -euo pipefail
SRC=$(cd "$(dirname "$0")/../native/fcitx5" && pwd)
BUILD=$(mktemp -d)
trap 'rm -rf "$BUILD"' EXIT
if [[ ${1:-} == --system ]]; then
    cmake -S "$SRC" -B "$BUILD" -DCMAKE_BUILD_TYPE=Release -DCMAKE_INSTALL_PREFIX=/usr
else
    cmake -S "$SRC" -B "$BUILD" -DCMAKE_BUILD_TYPE=Release -DCMAKE_INSTALL_PREFIX="$HOME/.local"
fi
cmake --build "$BUILD"
if [[ ${1:-} == --system ]]; then
    pkexec cmake --install "$BUILD"
else
    cmake --install "$BUILD"
    echo '若发行版不搜索用户插件目录，请使用 --system（会弹出 pkexec）。'
fi
echo '已安装 Fcitx5 中文桥接。重启 Fcitx5 后生效：'
echo 'qdbus org.fcitx.Fcitx5 /controller org.fcitx.Fcitx.Controller1.Restart'
