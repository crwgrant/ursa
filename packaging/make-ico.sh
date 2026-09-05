#!/usr/bin/env bash
# Build packaging/AppIcon.ico from the PNG sources. Run from Git Bash:
#   ./packaging/make-ico.sh
set -euo pipefail

cd "$(dirname "$0")"

python - <<'PY'
from pathlib import Path

from PIL import Image

here = Path.cwd()
small = Image.open(here / "AppIcon-small.png").convert("RGBA")
large = Image.open(here / "AppIcon.png").convert("RGBA")

# Same split as make-icns.sh: small art for 16–64, full art for 128–256.
sizes = (
    (16, small),
    (24, small),
    (32, small),
    (48, small),
    (64, small),
    (128, large),
    (256, large),
)
frames = [src.resize((size, size), Image.Resampling.LANCZOS) for size, src in sizes]
out = here / "AppIcon.ico"
frames[0].save(out, format="ICO", sizes=[(f.width, f.height) for f in frames], append_images=frames[1:])
print(f"wrote {out}")
PY
