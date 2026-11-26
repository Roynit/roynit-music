"""Cuts decorations out of sheets made by an image model (flat dark background, a flat blue disc where the avatar goes).

Writes keyed 512 x 512 pictures to assets/shop/_src/ai/. Every tile is found from the avatar discs, so tiles do not need
to be the same size.   python scripts/ingest_gemini.py
"""
import os
import sys

import numpy as np
from PIL import Image
from scipy.ndimage import binary_dilation, label

ROOT = os.path.join(os.path.dirname(__file__), "..")
OUT = os.path.join(ROOT, "assets", "shop", "_src", "ai")
IMAGES = r"C:\Users\HaiDang\AppData\Local\Temp\claude\b69ea16d089383a2\e78a375f-e193-4ead-ac15-723c935f8816\images"
AVATAR = 0.625  # avatar disc across, as a fraction of the picture
WATERMARKS = {"blossom_cream": (0.576, 0.66, 0.09), "wings_gold": (0.69, 0.71, 0.1)}  # the image model's little sparkle signature


def smooth(e0, e1, x):
    t = np.clip((x - e0) / (e1 - e0), 0, 1)
    return t * t * (3 - 2 * t)


def cut(sheet, cx, cy, hole_d, names, edge=(2.0, 9.0), fraction=AVATAR):
    a = np.asarray(sheet, float)
    bg = a[3:9, 3:9].reshape(-1, 3).mean(0)
    blue = (np.abs(a[..., 0] - 90) < 14) & (np.abs(a[..., 1] - 108) < 14) & (np.abs(a[..., 2] - 158) < 22)
    lab, _ = label(blue)
    sizes = np.bincount(lab.ravel())
    big = np.isin(lab, [i for i in range(1, len(sizes)) if sizes[i] > 3000])
    hole = binary_dilation(big, iterations=4)
    d_bg = np.sqrt(((a - bg) ** 2).sum(-1))
    alpha = smooth(edge[0], edge[1], d_bg)
    alpha[hole] = 0
    colour = np.clip((a - bg * (1 - alpha[..., None])) / np.maximum(alpha, 0.05)[..., None], 0, 255)
    rgba = np.dstack([colour, alpha * 255]).astype(np.uint8)
    full = Image.fromarray(rgba, "RGBA")
    side = int(round(hole_d / fraction))
    results = {}
    for name, (x, y, cell) in names.items():
        box = (int(round(x - side / 2)), int(round(y - side / 2)), int(round(x - side / 2)) + side, int(round(y - side / 2)) + side)
        tile = Image.new("RGBA", (side, side), (0, 0, 0, 0))
        tile.paste(full.crop(box), (0, 0))
        if cell:
            # stay inside this tile's own cell so a neighbour's edge never leaks in
            arr = np.asarray(tile).copy()
            xs = np.arange(side) + box[0]
            ys = np.arange(side) + box[1]
            keep = ((xs >= cell[0]) & (xs < cell[2]))[None, :] & ((ys >= cell[1]) & (ys < cell[3]))[:, None]
            arr[~keep, 3] = 0
            tile = Image.fromarray(arr, "RGBA")
        if name in WATERMARKS:
            wx, wy, wr = WATERMARKS[name]
            arr = np.asarray(tile).copy()
            yy, xx = np.mgrid[0:side, 0:side]
            arr[(np.abs(xx - wx * side) < wr * side) & (np.abs(yy - wy * side) < wr * side), 3] = 0
            tile = Image.fromarray(arr, "RGBA")
        tile.resize((512, 512), Image.LANCZOS).save(os.path.join(OUT, name + ".png"))
        results[name] = box
        print(name, box)
    return results


def main():
    os.makedirs(OUT, exist_ok=True)
    sheet_a = Image.open(os.path.join(IMAGES, "61.webp")).convert("RGB")
    pitch = sheet_a.width / 5
    row1, row2 = 170.0, 495.0
    xs = [162.3 + 317.1 * k for k in range(5)]
    cells = lambda k, row: (int(k * pitch), 0 if row == row1 else 336, int((k + 1) * pitch), 336 if row == row1 else 672)
    names = {
        "kitty_gray": (xs[0], row1, cells(0, row1)), "kitty_white": (xs[1], row1, cells(1, row1)),
        "kitty_ginger": (xs[2], row1, cells(2, row1)), "kitty_black": (xs[3], row1, cells(3, row1)),
        "night_gold": (xs[4], row1, cells(4, row1)),
        "moon_pink": (xs[0], row2, cells(0, row2)), "moon_blue": (xs[1], row2, cells(1, row2)),
        "blossom_pink": (xs[2], row2, cells(2, row2)), "blossom_plum": (xs[3], row2, cells(3, row2)), "blossom_cream": (xs[4], row2, cells(4, row2)),
    }
    cut(sheet_a, 0, 0, 202, names)
    sheet_b = Image.open(os.path.join(IMAGES, "62.webp")).convert("RGB")
    pitch_b = sheet_b.width / 3
    ab = np.asarray(sheet_b).astype(int)
    blue = (np.abs(ab[..., 0] - 90) < 14) & (np.abs(ab[..., 1] - 108) < 14) & (np.abs(ab[..., 2] - 158) < 22)
    lab, _ = label(blue)
    wings = {}
    for k, n in enumerate(["wings_white", "wings_rose", "wings_gold"]):
        # the wings hide the sides of the disc, so the centre comes from its top and bottom edges
        x0, x1 = int(k * pitch_b), int((k + 1) * pitch_b)
        sub = blue[:, x0:x1]
        ys, xs = np.where(sub)
        top, bottom = ys.min(), ys.max()
        h = bottom - top
        def chord(y0, y1):
            m = sub[y0:y1]
            cols = np.where(m.any(axis=0))[0]
            return (cols.min() + cols.max()) / 2
        cx = x0 + (chord(top + int(0.05 * h), top + int(0.2 * h)) + chord(bottom - int(0.2 * h), bottom - int(0.05 * h))) / 2
        wings[n] = (cx, (top + bottom) / 2, (x0, 0, x1, sheet_b.height))
        print(n, "centre", round(cx, 1), round((top + bottom) / 2, 1), "disc", h)
    cut(sheet_b, 0, 0, 244, wings, fraction=0.525)


if __name__ == "__main__":
    main()
