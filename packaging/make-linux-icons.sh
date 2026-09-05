#!/usr/bin/env bash
# Build packaging/linux/*.png from the PNG sources for cargo-packager.
# Linux packaging only copies PNG; AppIcon.icns and AppIcon.ico are skipped.
#   ./packaging/make-linux-icons.sh
set -euo pipefail

cd "$(dirname "$0")"
mkdir -p linux

py=
for candidate in python3 /usr/bin/python3 python; do
  if command -v "$candidate" >/dev/null && "$candidate" -c "from PIL import Image" 2>/dev/null; then
    py=$candidate
    break
  fi
done
if [[ -z $py ]]; then
  echo "need Python and Pillow (Arch: pacman -S python-pillow)" >&2
  exit 1
fi

"$py" - <<'PY'
from pathlib import Path

from PIL import Image

here = Path.cwd()
small = Image.open(here / "AppIcon-small.png").convert("RGBA")
large = Image.open(here / "AppIcon.png").convert("RGBA")

# Same art split as make-ico.sh / make-icns.sh, plus freedesktop sizes
# cargo-packager maps each PNG into usr/share/icons/hicolor/{WxH}/apps/.
sizes = (
    (16, small),
    (22, small),
    (24, small),
    (32, small),
    (48, small),
    (64, small),
    (128, large),
    (256, large),
    (512, large),
)
out_dir = here / "linux"
for size, src in sizes:
    dest = out_dir / f"{size}x{size}.png"
    src.resize((size, size), Image.Resampling.LANCZOS).save(dest, format="PNG")
    print(f"wrote {dest}")
PY
