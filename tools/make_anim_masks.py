import argparse
from pathlib import Path

import numpy as np
from PIL import Image

# girl
SHOULDER_BAND = (150, 215)
TORSO_BOTTOM_Y = 278
# long hair strands falling over the chest
HAIR_BOX = (100, 150, 215, 240)
HAIR_LUMA_MAX = 95
# chair back behind the left shoulder
CHAIR_MAX_X = 75
# laptop
LAPTOP_EDGE = ((214, 205), (176, 339))
LAPTOP_TOP_Y = 203
MIN_PIECE = 60
# hood ears
EAR_BAND_Y = 50
EAR_LUMA = (40, 120)
EAR_GROW_IN = 6

# cat
CAT_PATH_X = (62, 118)
CAT_BODY_BOTTOM_Y = 112
CAT_TAIL_TOP_Y = 116  
CAT_TAIL_TIP_MAX_X = 100 


def luma(rgb):
    return rgb[..., 0] * 0.299 + rgb[..., 1] * 0.587 + rgb[..., 2] * 0.114


def edges(rgba):
    g = luma(rgba[..., :3].astype(np.float32))
    gx = np.zeros_like(g)
    gy = np.zeros_like(g)
    gx[:, 1:-1] = g[:, 2:] - g[:, :-2]
    gy[1:-1, :] = g[2:, :] - g[:-2, :]
    return np.hypot(gx, gy)


def close_open(mask, r=1):
    def dil(m):
        out = m.copy()
        for dy in range(-r, r + 1):
            for dx in range(-r, r + 1):
                out |= np.roll(np.roll(m, dy, 0), dx, 1)
        return out

    def ero(m):
        return ~dil(~m)

    return dil(ero(ero(dil(mask))))


def min_path(cost):
    rows, cols = cost.shape
    acc = cost.copy()
    back = np.zeros((rows, cols), np.int8)
    for y in range(1, rows):
        prev = acc[y - 1]
        cand = np.stack([np.r_[np.inf, prev[:-1]], prev, np.r_[prev[1:], np.inf]])
        k = cand.argmin(axis=0)
        acc[y] += cand[k, np.arange(cols)]
        back[y] = k - 1
    path = np.zeros(rows, int)
    path[-1] = int(acc[-1].argmin())
    for y in range(rows - 1, 0, -1):
        path[y - 1] = path[y] + back[y, path[y]]
    return path


def edge_cost(rgba, opaque):
    c = 1.0 / (1.0 + edges(rgba))
    c[~opaque] = 0.05
    return c


def girl_torso(rgba):
    h, w = rgba.shape[:2]
    yy, xx = np.mgrid[0:h, 0:w]
    opaque = rgba[..., 3] > 0
    lo, hi = SHOULDER_BAND
    line = min_path(edge_cost(rgba, opaque)[lo:hi, :].T) + lo
    m = opaque & (yy > line[None, :]) & (yy < TORSO_BOTTOM_Y)
    lum = luma(rgba[..., :3].astype(np.float32))
    x0, y0, x1, y1 = HAIR_BOX
    m &= ~((lum < HAIR_LUMA_MAX) & (xx >= x0) & (xx < x1) & (yy >= y0) & (yy < y1))
    r, b = rgba[..., 0].astype(int), rgba[..., 2].astype(int)
    m &= ~((xx < CHAIR_MAX_X) & (b <= r + 15))
    (ax, ay), (bx, by) = LAPTOP_EDGE
    m &= ~((yy >= LAPTOP_TOP_Y) & (xx >= ax + (yy - ay) * (bx - ax) / (by - ay) - 1))
    return drop_small(close_open(m) & opaque, MIN_PIECE), line


def cat_body(rgba):
    h, w = rgba.shape[:2]
    opaque = rgba[..., 3] > 0
    lo, hi = CAT_PATH_X
    path = min_path(edge_cost(rgba, opaque)[:CAT_BODY_BOTTOM_Y, lo:hi]) + lo
    top = int(np.nonzero(opaque[:, hi:].any(axis=1))[0].min())
    m = np.zeros((h, w), bool)
    for y, x in enumerate(path):
        if y >= top:
            m[y, x + 1:] = True
    return m & opaque, path


def components(mask):
    h, w = mask.shape
    seen = np.zeros_like(mask)
    out = []
    for sy, sx in zip(*np.nonzero(mask)):
        if seen[sy, sx]:
            continue
        stack, comp = [(sy, sx)], []
        seen[sy, sx] = True
        while stack:
            y, x = stack.pop()
            comp.append((y, x))
            for ny, nx in ((y + 1, x), (y - 1, x), (y, x + 1), (y, x - 1)):
                if 0 <= ny < h and 0 <= nx < w and mask[ny, nx] and not seen[ny, nx]:
                    seen[ny, nx] = True
                    stack.append((ny, nx))
        out.append(comp)
    return out


def drop_small(mask, min_px):
    out = mask.copy()
    for comp in components(mask):
        if len(comp) < min_px:
            for y, x in comp:
                out[y, x] = False
    return out


def girl_ears(rgba):
    h, w = rgba.shape[:2]
    opaque = rgba[..., 3] > 0
    r, g, b = (rgba[..., i].astype(int) for i in range(3))
    lum = luma(rgba[..., :3].astype(np.float32))
    yy, xx = np.mgrid[0:h, 0:w]
    inner = opaque & (yy < EAR_BAND_Y) & (b > r + 30) & (lum > EAR_LUMA[0]) & (lum < EAR_LUMA[1])
    comps = sorted(components(inner), key=len, reverse=True)
    mid = w // 2
    left = next((c for c in comps if np.mean([x for _, x in c]) < mid), None)
    right = next((c for c in comps if np.mean([x for _, x in c]) >= mid), None)
    if left is None or right is None:
        raise SystemExit("make_anim_masks: could not find both inner-ear patches; check EAR_* constants")
    m = np.zeros((h, w), bool)
    boxes = []
    for comp, outward in ((left, -1), (right, 1)):
        ys = [y for y, _ in comp]
        xs = [x for _, x in comp]
        x0, x1, y1 = min(xs), max(xs) + 1, max(ys) + 1
        if outward < 0:
            x0, x1 = 0, x1 + EAR_GROW_IN
        else:
            x0, x1 = x0 - EAR_GROW_IN, w
        m[0:y1 + 2, x0:x1] = True
        boxes.append((x0, x1, y1))
    m &= opaque & (yy < EAR_BAND_Y)
    return m, boxes


def cat_tail(rgba):
    h, w = rgba.shape[:2]
    opaque = rgba[..., 3] > 0
    yy, xx = np.mgrid[0:h, 0:w]
    return opaque & (yy >= CAT_TAIL_TOP_Y) & (xx < CAT_TAIL_TIP_MAX_X)


def save_mask(m, path):
    out = np.zeros(m.shape + (4,), np.uint8)
    out[m] = (255, 255, 255, 255)
    Image.fromarray(out, "RGBA").save(path)


def preview(rgba, m, path, k=3):
    img = rgba.astype(np.float32).copy()
    img[..., :3][m] = img[..., :3][m] * 0.45 + np.array([255, 40, 40]) * 0.55
    img[..., 3] = 255
    Image.fromarray(img.astype(np.uint8), "RGBA").resize((m.shape[1] * k, m.shape[0] * k), Image.NEAREST).save(path)


def make(ready: Path, preview_dir: Path | None = None):
    girl = np.asarray(Image.open(ready / "girl.png").convert("RGBA"))
    torso, line = girl_torso(girl)
    save_mask(torso, ready / "girl_torso_mask.png")
    cat = np.asarray(Image.open(ready / "cat.png").convert("RGBA"))
    body, path = cat_body(cat)
    save_mask(body, ready / "cat_body_mask.png")
    ears, boxes = girl_ears(girl)
    save_mask(ears, ready / "girl_ears_mask.png")
    tail = cat_tail(cat)
    save_mask(tail, ready / "cat_tail_mask.png")
    print(f"  girl_torso_mask {int(torso.sum())} px (shoulder line y {line.min()}..{line.max()})   "
          f"cat_body_mask {int(body.sum())} px (head/body boundary x {path.min()}..{path.max()})")
    print(f"  girl_ears_mask {int(ears.sum())} px (ear boxes {boxes})   cat_tail_mask {int(tail.sum())} px")
    if preview_dir:
        preview(girl, torso, preview_dir / "mask_girl_torso.png")
        preview(cat, body, preview_dir / "mask_cat_body.png", 4)
        preview(girl, ears, preview_dir / "mask_girl_ears.png")
        preview(cat, tail, preview_dir / "mask_cat_tail.png", 4)


def main():
    root = Path(__file__).resolve().parent.parent
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--ready", type=Path, default=root / "assets" / "ready")
    ap.add_argument("--preview", type=Path, help="also write red-tinted previews here")
    args = ap.parse_args()
    make(args.ready, args.preview)


if __name__ == "__main__":
    main()
