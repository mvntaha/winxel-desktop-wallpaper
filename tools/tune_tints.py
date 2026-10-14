import argparse
from pathlib import Path

import numpy as np
from PIL import Image

PLATES = ["night", "dawn", "day", "dusk"]
REFS = {"plant": (115, 280, 190, 395), "mug": (638, 360, 705, 445)}


def luma(rgb):
    return rgb[..., 0] * 0.299 + rgb[..., 1] * 0.587 + rgb[..., 2] * 0.114


def ref_pixels(img: np.ndarray) -> np.ndarray:
    return np.concatenate([img[y0:y1, x0:x1].reshape(-1, 3) for x0, y0, x1, y1 in REFS.values()])


def main():
    root = Path(__file__).resolve().parent.parent
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--ready", type=Path, default=root / "assets" / "ready")
    ap.add_argument("--chroma", type=float, default=0.5, help="0 = grey tint, 1 = full measured cast")
    ap.add_argument("--max-luma", type=float, default=1.0, help="cap on tint brightness")
    args = ap.parse_args()

    plates = {p: np.asarray(Image.open(args.ready / f"room_{p}.png").convert("RGB"), np.float32) for p in PLATES}
    day = ref_pixels(plates["day"])
    sprites = []
    for name in ["girl", "cat"]:
        a = np.asarray(Image.open(args.ready / f"{name}.png").convert("RGBA"), np.float32)
        sprites.append(a[a[..., 3] > 0][:, :3])
    sprite_px = np.concatenate(sprites)
    day_ratio = luma(sprite_px).mean() / luma(day).mean()

    print(f"sprite/reference brightness at day (untinted): {day_ratio:.2f}\n")
    print(f"{'plate':6s} {'tint (r, g, b)':24s} {'ref luma':>8s} {'sprite luma':>11s} {'ratio':>6s}")
    for p in PLATES:
        ref = ref_pixels(plates[p])
        cast = np.median(ref / np.maximum(day, 8), axis=0)
        grey = luma(cast)
        tint = grey + (cast - grey) * args.chroma
        want = day_ratio * luma(ref).mean()
        for _ in range(4):
            got = luma(np.clip(sprite_px * tint, 0, 255)).mean()
            tint = tint * want / got
        tint = np.minimum(tint * min(1.0, args.max_luma / luma(tint)), 1.0)
        tinted = np.clip(sprite_px * tint, 0, 255)
        print(f"{p:6s} [{tint[0]:.2f}, {tint[1]:.2f}, {tint[2]:.2f}]{'':4s} "
              f"{luma(ref).mean():8.1f} {luma(tinted).mean():11.1f} {luma(tinted).mean() / luma(ref).mean():6.2f}")
        print(f"       const {p.upper()}_TINT: [f32; 3] = [{tint[0]:.2f}, {tint[1]:.2f}, {tint[2]:.2f}];")


if __name__ == "__main__":
    main()
