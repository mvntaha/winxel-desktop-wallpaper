import argparse
import json
from pathlib import Path

import numpy as np
from PIL import Image, ImageDraw

SCREEN = (1920, 1080)
CROP = (560, 120, 1360, 760)  # screen-space region: window, skyline, desk, girl


def load_sprite(ready: Path, name: str):
    meta = json.loads((ready / f"{name}.json").read_text())
    return Image.open(ready / f"{name}.png").convert("RGBA"), (meta["x"], meta["y"])


def composite(ready: Path, plate: str, sprites) -> Image.Image:
    base = Image.open(ready / f"room_{plate}.png").convert("RGBA")
    for s in sprites:
        img, pos = load_sprite(ready, s)
        base.alpha_composite(img, pos)
    return base


def to_screen(img: Image.Image) -> Image.Image:
    return img.resize(SCREEN, Image.NEAREST)


def gradient_energy(img: Image.Image) -> float:
    g = np.asarray(img.convert("L"), dtype=np.float32)
    return float(np.abs(np.diff(g, axis=1)).mean() + np.abs(np.diff(g, axis=0)).mean())


def label(img: Image.Image, text: str) -> Image.Image:
    out = img.convert("RGB")
    d = ImageDraw.Draw(out)
    d.rectangle((0, 0, 8 * len(text) + 16, 24), fill=(0, 0, 0))
    d.text((8, 6), text, fill=(255, 255, 255))
    return out


def main():
    root = Path(__file__).resolve().parent.parent
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--ready", type=Path, default=root / "assets" / "ready")
    ap.add_argument("--compare", type=Path, help="dir with a 640x360 conversion for the sharpness check")
    args = ap.parse_args()

    comp = composite(args.ready, "night", ["girl", "cat"])
    comp.save(args.ready / "check_composite.png")
    to_screen(composite(args.ready, "night", ["girl"])).save(args.ready / "check_girl.png")
    print(f"wrote check_composite.png {comp.size}, check_girl.png {SCREEN}")

    if args.compare:
        hi = to_screen(Image.open(args.ready / "room_night.png").convert("RGB")).crop(CROP)
        lo = to_screen(Image.open(args.compare / "room_night.png").convert("RGB")).crop(CROP)
        e_lo, e_hi = gradient_energy(lo), gradient_energy(hi)
        w, h = hi.size
        sheet = Image.new("RGB", (w * 2 + 8, h), (0, 0, 0))
        sheet.paste(label(lo, f"640x360 x3  grad {e_lo:.2f}"), (0, 0))
        sheet.paste(label(hi, f"960x540 x2  grad {e_hi:.2f}"), (w + 8, 0))
        sheet.save(args.ready / "check_sharpness.png")
        print(f"wrote check_sharpness.png  gradient energy at 1080p: 640x3={e_lo:.2f}  960x2={e_hi:.2f}"
              f"  ({(e_hi / e_lo - 1) * 100:+.0f}%)")


if __name__ == "__main__":
    main()
