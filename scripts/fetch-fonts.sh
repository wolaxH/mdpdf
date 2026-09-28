#!/bin/sh
# 下載內嵌用的 CJK 字型（Noto Sans TC，OFL 授權）到 assets/fonts/
set -eu
cd "$(dirname "$0")/../assets/fonts"
base=https://github.com/notofonts/noto-cjk/raw/main/Sans/SubsetOTF/TC
for w in Regular Bold; do
    [ -f "NotoSansTC-$w.otf" ] || curl -fL --retry 3 -o "NotoSansTC-$w.otf" "$base/NotoSansTC-$w.otf"
done
[ -f LICENSE ] || curl -fL --retry 3 -o LICENSE https://github.com/notofonts/noto-cjk/raw/main/Sans/LICENSE
