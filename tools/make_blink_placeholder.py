import argparse
from pathlib import Path

import numpy as np
from PIL import Image

OVERLAY_AT = (122, 94)          # top-left of the overlay in girl.png space
OVERLAY_SIZE = (78, 22)
EYES = [(124, 151), (174, 198)]  # x ranges of the left and right eye
LID_ROWS = (97, 115)            # rows covered by skin (upper lid down to lower lid)
LASH_Y = 106                    # closed-eye line, bowed down 1 px at the middle
SKIN_SAMPLE = (160, 110)        # cheek pixel between the eyes
LASH_RGB = (40, 26, 40)


def make(ready: Path):
    girl = np.asarray(Image.open(ready / "girl.png").convert("RGBA")).copy()
    skin = girl[SKIN_SAMPLE[1], SKIN_SAMPLE[0]].copy()
    ox, oy = OVERLAY_AT
    w, h = OVERLAY_SIZE
    out = np.zeros((h, w, 4), np.uint8)
    for x0, x1 in EYES:
        for x in range(x0, x1):
            for y in range(*LID_ROWS):
                out[y - oy, x - ox] = skin
            t = (x - x0) / max(1, x1 - x0 - 1)
            bow = 1 if 0.25 < t < 0.75 else 0
            for dy in (0, 1):
                out[LASH_Y + bow + dy - oy, x - ox] = (*LASH_RGB, 255)
    Image.fromarray(out, "RGBA").save(ready / "girl_blink.png")
    print(f"  wrote girl_blink.png {w}x{h} at {OVERLAY_AT} (PLACEHOLDER art)")


def main():
    root = Path(__file__).resolve().parent.parent
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--ready", type=Path, default=root / "assets" / "ready")
    make(ap.parse_args().ready)


if __name__ == "__main__":
    main()
