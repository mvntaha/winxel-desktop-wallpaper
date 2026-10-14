import argparse
import json
import sys
from pathlib import Path

import numpy as np
from PIL import Image

import make_anim_masks

PLATES = ["night", "dawn", "day", "dusk"]

# the cat sits at the right end of the desk; the girl stays where her raw puts her for now.
SPRITE_LAYOUT = {
    "cat": dict(scale=0.5, place=(1048, 455)),
}
SPRITES = ["girl", "cat"]
MAX_SPRITE_COLORS = 48
ALIGN_WARN_PX = 1.0  # in target pixels
RAW_EXTS = (".png", ".jpg", ".jpeg", ".webp")


def find_raw(raw_dir: Path, stem: str) -> Path | None:
    for ext in RAW_EXTS:
        if (raw_dir / (stem + ext)).exists():
            return raw_dir / (stem + ext)
    return None


# geometry

def crop_box(w: int, h: int) -> tuple[int, int, int, int]:
    if w * 9 == h * 16:
        return 0, 0, w, h
    if w * 9 > h * 16:  # too wide
        nw = h * 16 // 9
        nw -= nw % 16
        nh = nw * 9 // 16
    else:  # too tall
        nh = w * 9 // 16
        nh -= nh % 9
        nw = nh * 16 // 9
    return (w - nw) // 2, (h - nh) // 2, nw, nh


def crop_16x9(img: Image.Image) -> Image.Image:
    x0, y0, w, h = crop_box(*img.size)
    return img.crop((x0, y0, x0 + w, y0 + h))


def luma(rgb: np.ndarray) -> np.ndarray:
    return rgb[..., 0] * 0.299 + rgb[..., 1] * 0.587 + rgb[..., 2] * 0.114


# block grid

def _edge_profile(gray: np.ndarray, axis: int) -> np.ndarray:
    d = np.abs(np.diff(gray, axis=axis))
    return d.mean(axis=0 if axis == 1 else 1)


def _detect_period(profile: np.ndarray, lo=1.5, hi=16.0):
    profile = np.clip(profile - np.median(profile), 0, None)
    n = len(profile)
    mean = profile.mean() + 1e-9
    results = []
    for p in np.arange(lo, hi + 1e-9, 0.02):
        best_r, best_ph = 0.0, 0.0
        for ph in np.arange(0.0, p, 0.25):
            idx = np.round(ph + np.arange(0, n / p) * p).astype(int)
            idx = idx[(idx >= 0) & (idx < n)]
            if len(idx) < 8:
                continue
            r = profile[idx].mean() / mean
            if r > best_r:
                best_r, best_ph = r, ph
        results.append((p, best_ph, (best_r - 1) / p))
    if not results:
        return None
    p, ph, ex = max(results, key=lambda t: t[2])
    return float(p), float(ph), float(ex)


def detect_block_grid(rgb: np.ndarray, mask: np.ndarray | None = None):
    gray = luma(rgb.astype(np.float32))
    if mask is not None:
        gray = np.where(mask, gray, gray[mask].mean() if mask.any() else 0)
    px = _detect_period(_edge_profile(gray, axis=1))
    py = _detect_period(_edge_profile(gray, axis=0))
    if px is None or py is None:
        return None
    (pxv, phx, rx), (pyv, phy, ry) = px, py
    grid = dict(px=pxv, py=pyv, phx=(phx + 1) % pxv, phy=(phy + 1) % pyv, rx=rx, ry=ry)
    convincing = rx > 0.2 and ry > 0.2 and abs(pxv - pyv) / max(pxv, pyv) < 0.15
    grid["ok"] = bool(convincing)
    return grid


def sample_coords(n_out: int, src_len: int, period: float, phase: float) -> np.ndarray:
    scale = src_len / n_out
    s = (np.arange(n_out) + 0.5) * scale
    k = np.floor((s - phase) / period)
    c = phase + (k + 0.5) * period
    return np.clip(np.round(c - 0.5).astype(int), 0, src_len - 1)


def downscale(arr: np.ndarray, out_w: int, out_h: int, grid) -> tuple[np.ndarray, str]:
    h, w = arr.shape[:2]
    scale = w / out_w
    if grid and grid["ok"] and grid["px"] >= scale * 0.85:
        xs = sample_coords(out_w, w, grid["px"], grid["phx"])
        ys = sample_coords(out_h, h, grid["py"], grid["phy"])
        return arr[ys][:, xs], f"block-center (block {grid['px']:.2f}x{grid['py']:.2f}px)"
    mode = "RGBA" if arr.shape[2] == 4 else "RGB"
    img = Image.fromarray(arr, mode).resize((out_w, out_h), Image.BOX)
    return np.asarray(img), "area"


# sprites

def key_magenta(rgb: np.ndarray, tol: float, soft: float) -> tuple[np.ndarray, np.ndarray]:
    f = rgb.astype(np.float32)
    r, g, b = f[..., 0], f[..., 1], f[..., 2]
    dist = np.sqrt((r - 255) ** 2 + g ** 2 + (b - 255) ** 2)
    alpha = np.clip((dist - tol) / max(soft, 1e-6), 0, 1)
    m = np.minimum(r, b)
    spill = np.clip(m - g, 0, None)
    near = (alpha < 1) | (_dilate(alpha < 1, 2))
    amt = np.where(near, spill, 0)
    out = f.copy()
    out[..., 0] -= amt
    out[..., 2] -= amt
    return np.clip(out, 0, 255), alpha


def _dilate(mask: np.ndarray, r: int) -> np.ndarray:
    out = mask.copy()
    for dy in range(-r, r + 1):
        for dx in range(-r, r + 1):
            out |= np.roll(np.roll(mask, dy, 0), dx, 1)
    return out


def quantize_opaque(rgba: np.ndarray, max_colors: int) -> tuple[np.ndarray, int]:
    opaque = rgba[..., 3] > 0
    if not opaque.any():
        return rgba, 0
    px = rgba[opaque][:, :3]
    strip = Image.fromarray(px.reshape(1, -1, 3), "RGB")
    q = strip.quantize(colors=max_colors, method=Image.Quantize.MEDIANCUT, dither=Image.Dither.NONE)
    qrgb = np.asarray(q.convert("RGB")).reshape(-1, 3)
    out = rgba.copy()
    out[opaque, :3] = qrgb
    return out, len(np.unique(qrgb, axis=0))


def resize_premultiplied(rgba: np.ndarray, w: int, h: int) -> np.ndarray:
    pre = rgba.astype(np.float32)
    pre[..., :3] *= pre[..., 3:4] / 255
    ch = [np.asarray(Image.fromarray(pre[..., c], "F").resize((w, h), Image.BOX)) for c in range(4)]
    a = ch[3]
    rgb = np.dstack(ch[:3]) / np.maximum(a, 1e-3)[..., None] * 255
    return np.dstack([np.clip(rgb, 0, 255), a]).round().astype(np.uint8)


def sprite_geometry(alpha: np.ndarray, raw_w: int, raw_h: int, out_w: int, scale: float, place):
    ys, xs = np.nonzero(alpha > 0.5)
    if len(xs) == 0:
        return None
    cx0, cy0, cw, _ = crop_box(raw_w, raw_h)
    f = out_w / cw  # raw px -> output px
    bx0, bx1, by0, by1 = int(xs.min()), int(xs.max()) + 1, int(ys.min()), int(ys.max()) + 1
    return dict(bbox=(bx0, by0, bx1, by1), f=f, crop0=(cx0, cy0), scale=scale,
                size=(max(1, round((bx1 - bx0) * scale * f)), max(1, round((by1 - by0) * scale * f))),
                place=place if place else (bx0, by0))


def render_sprite(keyed: np.ndarray, alpha: np.ndarray, geo) -> np.ndarray:
    bx0, by0, bx1, by1 = geo["bbox"]
    rgba = np.dstack([keyed, alpha * 255]).round().astype(np.uint8)[by0:by1, bx0:bx1]
    small = resize_premultiplied(rgba, *geo["size"])
    small[..., 3] = np.where(small[..., 3] >= 128, 255, 0)
    small[small[..., 3] == 0] = 0
    return small


def convert_sprite(path: Path, out_dir: Path, name: str, out_w: int, out_h: int,
                   tol: float, soft: float, scale: float, place):
    rgb = np.asarray(Image.open(path).convert("RGB"))
    keyed, alpha = key_magenta(rgb, tol, soft)
    geo = sprite_geometry(alpha, rgb.shape[1], rgb.shape[0], out_w, scale, place)
    if geo is None:
        print(f"  !! {name}: nothing left after keying (tol={tol})")
        return None
    plain = render_sprite(keyed, alpha, geo)
    small, ncolors = quantize_opaque(plain, MAX_SPRITE_COLORS)

    (px, py), (cx0, cy0), f = geo["place"], geo["crop0"], geo["f"]
    ys, xs = np.nonzero(small[..., 3])
    tx0, tx1, ty0, ty1 = int(xs.min()), int(xs.max()) + 1, int(ys.min()), int(ys.max()) + 1
    geo["trim"] = (tx0, ty0, tx1, ty1)
    x = round((px - cx0) * f) + tx0
    y = round((py - cy0) * f) + ty0
    crop = small[ty0:ty1, tx0:tx1]
    Image.fromarray(crop, "RGBA").save(out_dir / f"{name}.png", optimize=True)
    meta = dict(x=int(x), y=int(y), w=int(tx1 - tx0), h=int(ty1 - ty0), space=f"{out_w}x{out_h}",
                colors=ncolors, source=path.name, scale=scale, place_src=[int(px), int(py)])
    (out_dir / f"{name}.json").write_text(json.dumps(meta, indent=2) + "\n")
    bx0, by0, bx1, by1 = geo["bbox"]
    src_box = f"src ({px},{py})-({px + round((bx1 - bx0) * scale)},{py + round((by1 - by0) * scale)})"
    print(f"  {name:6s} scale {scale:<5g} {src_box:30s} -> out ({x},{y}) {tx1 - tx0}x{ty1 - ty0}  colors={ncolors}")
    return meta, geo, plain[ty0:ty1, tx0:tx1]


# blink

BLINK_LUMA_DIFF = 40
BLINK_MIN_PIECE = 6
BLINK_GROW = 1


def set_blink_at(anim_json: Path, at, image="girl_blink.png"):
    import re
    text = anim_json.read_text(encoding="utf-8")
    pat = re.compile(r'("image":\s*"' + re.escape(image) + r'",\s*"at":\s*)\[\s*-?\d+\s*,\s*-?\d+\s*\]')
    new, n = pat.subn(lambda m: f"{m.group(1)}[{at[0]}, {at[1]}]", text)
    if n != 1:
        print(f"  !! {anim_json.name}: blink entry with \"image\": \"{image}\" not found; set \"at\": {list(at)} by hand")
        return
    if new != text:
        anim_json.write_text(new, encoding="utf-8")
    print(f"  {anim_json.name}: blink at {list(at)}")


def convert_blink(blink_path: Path, girl_path: Path, girl_geo, girl_plain: np.ndarray, out_dir: Path,
                  tol: float, soft: float, anim_json: Path):
    girl_rgb = np.asarray(Image.open(girl_path).convert("RGB"))
    blink_rgb = np.asarray(Image.open(blink_path).convert("RGB"))
    if blink_rgb.shape != girl_rgb.shape:
        print(f"  !! {blink_path.name} is {blink_rgb.shape[1]}x{blink_rgb.shape[0]}, girl_raw is "
              f"{girl_rgb.shape[1]}x{girl_rgb.shape[0]}; skipped (placeholder kept)")
        return False
    bx0, by0, bx1, by1 = girl_geo["bbox"]
    dx, dy, peak = phase_correlate(edge_map(girl_rgb[by0:by1, bx0:bx1]), edge_map(blink_rgb[by0:by1, bx0:bx1]))
    sx, sy = round(dx), round(dy)
    blink_rgb = np.roll(np.roll(blink_rgb, sy, axis=0), sx, axis=1)
    keyed, alpha = key_magenta(blink_rgb, tol, soft)
    tx0, ty0, tx1, ty1 = girl_geo["trim"]
    plain = render_sprite(keyed, alpha, girl_geo)[ty0:ty1, tx0:tx1]

    both = (plain[..., 3] > 0) & (girl_plain[..., 3] > 0)
    diff = np.abs(luma(plain[..., :3].astype(np.float32)) - luma(girl_plain[..., :3].astype(np.float32)))
    changed = make_anim_masks.drop_small(both & (diff > BLINK_LUMA_DIFF), BLINK_MIN_PIECE)
    if not changed.any():
        print(f"  !! {blink_path.name}: no difference from the girl found; placeholder kept")
        return False
    grown = changed.copy()
    for oy in range(-BLINK_GROW, BLINK_GROW + 1):
        for ox in range(-BLINK_GROW, BLINK_GROW + 1):
            grown |= np.roll(np.roll(changed, oy, 0), ox, 1)
    grown &= plain[..., 3] > 0
    ys, xs = np.nonzero(grown)
    x0, x1, y0, y1 = int(xs.min()), int(xs.max()) + 1, int(ys.min()), int(ys.max()) + 1
    overlay = np.zeros((y1 - y0, x1 - x0, 4), np.uint8)
    region = grown[y0:y1, x0:x1]
    overlay[region] = plain[y0:y1, x0:x1][region]
    overlay, ncolors = quantize_opaque(overlay, MAX_SPRITE_COLORS)
    Image.fromarray(overlay, "RGBA").save(out_dir / "girl_blink.png", optimize=True)
    print(f"  blink  aligned by ({sx:+d},{sy:+d}) raw px (peak {peak:.2f}); changed {int(changed.sum())} px; "
          f"overlay {x1 - x0}x{y1 - y0} at ({x0},{y0}) in girl.png space, colors={ncolors}")
    set_blink_at(anim_json, (x0, y0))
    return True


# alignment

def edge_map(rgb: np.ndarray) -> np.ndarray:
    g = luma(rgb.astype(np.float32))
    gx = np.zeros_like(g)
    gy = np.zeros_like(g)
    gx[:, 1:-1] = g[:, 2:] - g[:, :-2]
    gy[1:-1, :] = g[2:, :] - g[:-2, :]
    e = np.hypot(gx, gy)
    return (e - e.mean()) / (e.std() + 1e-9)


def phase_correlate(a: np.ndarray, b: np.ndarray) -> tuple[float, float, float]:
    h, w = a.shape
    win = np.outer(np.hanning(h), np.hanning(w))
    fa = np.fft.fft2(a * win)
    fb = np.fft.fft2(b * win)
    cross = fa * np.conj(fb)
    cross /= np.abs(cross) + 1e-9
    corr = np.real(np.fft.ifft2(cross))
    py, px = np.unravel_index(np.argmax(corr), corr.shape)

    def sub(c_m, c_0, c_p):
        d = c_m - 2 * c_0 + c_p
        return 0.0 if abs(d) < 1e-12 else 0.5 * (c_m - c_p) / d

    dx = px + sub(corr[py, (px - 1) % w], corr[py, px], corr[py, (px + 1) % w])
    dy = py + sub(corr[(py - 1) % h, px], corr[py, px], corr[(py + 1) % h, px])
    if dx > w / 2:
        dx -= w
    if dy > h / 2:
        dy -= h
    return float(dx), float(dy), float(corr[py, px])


# main

def main() -> int:
    root = Path(__file__).resolve().parent.parent
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--res", default="960x540", help="target resolution WxH (default 960x540)")
    ap.add_argument("--raw", type=Path, default=root / "assets" / "raw")
    ap.add_argument("--out", type=Path, default=root / "assets" / "ready")
    ap.add_argument("--key-tol", type=float, default=110, help="RGB distance from #FF00FF that is fully keyed")
    ap.add_argument("--key-soft", type=float, default=40, help="soft band above --key-tol")
    ap.add_argument("--no-grid", action="store_true", help="skip block detection, always area-downscale")
    ap.add_argument("--scale", action="append", default=[], metavar="NAME=F",
                    help="sprite size relative to its raw, e.g. cat=0.5 (repeatable)")
    ap.add_argument("--place", action="append", default=[], metavar="NAME=X,Y",
                    help="sprite bbox top-left in raw/source px, e.g. cat=1048,455 (repeatable)")
    ap.add_argument("--anim", type=Path, default=root / "assets" / "anim.json",
                    help="anim.json whose blink \"at\" is updated")
    args = ap.parse_args()

    out_w, out_h = (int(v) for v in args.res.lower().split("x"))
    if out_w * 9 != out_h * 16:
        print(f"--res {args.res} is not 16:9")
        return 2
    args.out.mkdir(parents=True, exist_ok=True)

    stems = [f"room_{p}" for p in PLATES] + [f"{s}_raw" for s in SPRITES]
    raw = {s: find_raw(args.raw, s) for s in stems}
    missing = [s for s, path in raw.items() if path is None]
    if missing:
        print(f"Missing in {args.raw} ({'/'.join(RAW_EXTS)}): {', '.join(missing)}")
        return 1

    print(f"Target {out_w}x{out_h}  raw={args.raw}  out={args.out}\n")
    print("Plates:")
    full = {}
    for p in PLATES:
        src = Image.open(raw[f"room_{p}"]).convert("RGB")
        img = crop_16x9(src)
        rgb = np.asarray(img)
        full[p] = rgb
        grid = None if args.no_grid else detect_block_grid(rgb)
        small, method = downscale(rgb, out_w, out_h, grid)
        Image.fromarray(small, "RGB").save(args.out / f"room_{p}.png", optimize=True)
        gtxt = (f"grid {grid['px']:.2f}x{grid['py']:.2f} excess {grid['rx']:.2f}/{grid['ry']:.2f}"
                if grid else "grid -")
        print(f"  {p:6s} {src.size[0]}x{src.size[1]} -> crop {img.size[0]}x{img.size[1]}  "
              f"{method:34s} {gtxt}")

    layout = {s: dict(SPRITE_LAYOUT.get(s, {})) for s in SPRITES}
    for opt in args.scale:
        n, v = opt.split("=", 1)
        layout.setdefault(n, {})["scale"] = float(v)
    for opt in args.place:
        n, v = opt.split("=", 1)
        layout.setdefault(n, {})["place"] = tuple(int(t) for t in v.split(","))
    print("\nSprites:")
    converted = {}
    for s in SPRITES:
        converted[s] = convert_sprite(raw[f"{s}_raw"], args.out, s, out_w, out_h, args.key_tol, args.key_soft,
                                      layout[s].get("scale", 1.0), layout[s].get("place"))

    print("\nBlink:")
    blink_raw = find_raw(args.raw, "girl_blink_raw")
    done = False
    if blink_raw and converted.get("girl"):
        _, geo, plain = converted["girl"]
        done = convert_blink(blink_raw, raw["girl_raw"], geo, plain, args.out, args.key_tol, args.key_soft, args.anim)
    if not done:
        if not blink_raw:
            print(f"  no girl_blink_raw in {args.raw}: using the PLACEHOLDER blink")
        import make_blink_placeholder
        make_blink_placeholder.make(args.out)
        set_blink_at(args.anim, make_blink_placeholder.OVERLAY_AT)

    print("\nAnimation masks:")
    make_anim_masks.make(args.out)

    print("\nAlignment (phase correlation on edge maps, vs night, in target px):")
    ref = edge_map(full["night"])
    sx = full["night"].shape[1] / out_w
    worst = 0.0
    report = {}
    for p in PLATES[1:]:
        if full[p].shape != full["night"].shape:
            print(f"  {p:6s} size mismatch {full[p].shape[1::-1]} vs {full['night'].shape[1::-1]}")
            worst = float("inf")
            continue
        dx, dy, peak = phase_correlate(ref, edge_map(full[p]))
        dx, dy = dx / sx, dy / sx
        mag = (dx * dx + dy * dy) ** 0.5
        worst = max(worst, mag)
        report[p] = dict(dx=round(dx, 3), dy=round(dy, 3), peak=round(peak, 4))
        flag = "  <-- MISALIGNED" if mag > ALIGN_WARN_PX else ""
        print(f"  {p:6s} dx={dx:+7.3f} dy={dy:+7.3f}  |{mag:.3f}|  peak={peak:.3f}{flag}")
    print(f"\nMax shift: {worst:.3f} target px  "
          f"({'OK' if worst <= ALIGN_WARN_PX else 'over ' + str(ALIGN_WARN_PX) + ' px threshold'})")
    report["max_shift_px"] = round(worst, 3)
    (args.out / "alignment.json").write_text(json.dumps(report, indent=2) + "\n")
    return 0


if __name__ == "__main__":
    sys.exit(main())
