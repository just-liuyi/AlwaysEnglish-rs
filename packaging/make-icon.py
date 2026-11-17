"""Regenerates the icon files from a single square source image.

    python packaging/make-icon.py [source.png]

`source.png` defaults to `assets/icon-source.png` and should be at least 256x256
(1024x1024 is best, the small sizes are downsampled from it).

Writes:
  assets/icon.png     256x256, used by the window, the tray icon and the UI
  assets/favicon.ico  16..256, embedded into the executable and used by the installer
"""

import sys
from pathlib import Path

from PIL import Image

ROOT = Path(__file__).resolve().parent.parent
SIZES = [(16, 16), (20, 20), (24, 24), (32, 32), (40, 40), (48, 48), (64, 64), (128, 128), (256, 256)]


def main() -> None:
    src = Path(sys.argv[1]) if len(sys.argv) > 1 else ROOT / "assets" / "icon-source.png"
    img = Image.open(src).convert("RGBA")
    side = min(img.size)
    left, top = (img.width - side) // 2, (img.height - side) // 2
    img = img.crop((left, top, left + side, top + side))
    if side < 256:
        sys.exit(f"{src} is only {side}px, at least 256px is needed")

    img.resize((256, 256), Image.LANCZOS).save(ROOT / "assets" / "icon.png")
    img.save(ROOT / "assets" / "favicon.ico", format="ICO", sizes=SIZES)
    print(f"icon.png 256x256, favicon.ico {', '.join(f'{w}x{h}' for w, h in SIZES)}")


if __name__ == "__main__":
    main()
