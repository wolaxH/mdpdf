#!/bin/sh
# Download the CJK fonts to embed (Noto Sans TC, OFL) into assets/fonts/.
# Pinned to a release and verified by checksum, like packaging/aur/mdpdf-git/PKGBUILD.
set -eu
cd "$(dirname "$0")/../assets/fonts"
base=https://github.com/notofonts/noto-cjk/raw/Sans2.004
fetch() {
    [ -f "$1" ] || curl -fL --retry 3 -o "$1" "$2"
}
fetch NotoSansTC-Regular.otf "$base/Sans/SubsetOTF/TC/NotoSansTC-Regular.otf"
fetch NotoSansTC-Bold.otf "$base/Sans/SubsetOTF/TC/NotoSansTC-Bold.otf"
fetch LICENSE "$base/LICENSE"
sha256sum -c <<'SUMS'
5bab0cb3c1cf89dde07c4a95a4054b195afbcfe784d69d75c340780712237537  NotoSansTC-Regular.otf
55420b259eb119bf5f2a0aadba10cf9d736c12d64ab93e78546d69ef5f43558b  NotoSansTC-Bold.otf
6a73f9541c2de74158c0e7cf6b0a58ef774f5a780bf191f2d7ec9cc53efe2bf2  LICENSE
SUMS
