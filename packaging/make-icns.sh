#!/bin/zsh
set -euo pipefail

cd "${0:A:h}"
tmp="$(mktemp -d /tmp/Skiff.XXXXXX)"
iconset="$tmp/AppIcon.iconset"
mkdir -p "$iconset"
trap 'rm -rf "$tmp"' EXIT

sips -z 16 16     AppIcon-small.png --out "$iconset/icon_16x16.png" >/dev/null
sips -z 32 32     AppIcon-small.png --out "$iconset/icon_16x16@2x.png" >/dev/null
sips -z 32 32     AppIcon-small.png --out "$iconset/icon_32x32.png" >/dev/null
sips -z 64 64     AppIcon-small.png --out "$iconset/icon_32x32@2x.png" >/dev/null
sips -z 128 128   AppIcon.png --out "$iconset/icon_128x128.png" >/dev/null
sips -z 256 256   AppIcon.png --out "$iconset/icon_128x128@2x.png" >/dev/null
sips -z 256 256   AppIcon.png --out "$iconset/icon_256x256.png" >/dev/null
sips -z 512 512   AppIcon.png --out "$iconset/icon_256x256@2x.png" >/dev/null
sips -z 512 512   AppIcon.png --out "$iconset/icon_512x512.png" >/dev/null
sips -z 1024 1024 AppIcon.png --out "$iconset/icon_512x512@2x.png" >/dev/null

iconutil -c icns "$iconset" -o AppIcon.icns
echo "wrote $PWD/AppIcon.icns"
