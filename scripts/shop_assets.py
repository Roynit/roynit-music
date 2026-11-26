"""Draws the Roy Music shop art: avatar decorations, nameplates, profile frames and profile-effect sprites.

Everything is original and procedural (signed-distance shapes, gradients, blurred glows), rendered at the final size
with analytic anti-aliasing. Run it to rebuild assets/shop/, the catalog the Worker reads, and the copies the apps bundle:

    python scripts/shop_assets.py            # rebuild everything
    python scripts/shop_assets.py --sheet    # also write assets/shop/_preview.png (a contact sheet to look at)
"""
import argparse
import colorsys
import json
import math
from pathlib import Path

import numpy as np
from PIL import Image, ImageDraw, ImageFilter
from scipy.ndimage import gaussian_filter

ROOT = Path(__file__).resolve().parent.parent
OUT = ROOT / "assets" / "shop"
DECO = 512      # decoration canvas; the avatar circle is AVATAR px across, centred
AVATAR = 320
rng = np.random.default_rng(7)


from shop_kit import *  # noqa: F401,F403
import shop_art_pro as pro
import shop_art_ideas as ideas


# ---------------------------------------------------------------- avatar decorations (512 x 512, avatar in the middle)

def deco_neon(c1, c2):
    x, y = grid(DECO)
    cx = cy = DECO / 2
    r = np.hypot(x - cx, y - cy)
    th = np.arctan2(y - cy, x - cx)
    t = (th / (2 * math.pi)) % 1
    col = ramp([c1, c2, c1], t)
    ring = paint(aa(np.abs(r - 184) - 6), col)
    lay = with_glow(ring, 11, 1.15, 0.5)
    hi = paint(aa(np.abs(r - 181) - 1.4), np.array([1, 1, 1.0]), 0.65)
    over(lay, hi)
    dashes = (np.sin(th * 28) > 0.15).astype(float)
    over(lay, paint(aa(np.abs(r - 211) - 1.6) * dashes, col, 0.8))
    over(lay, paint(aa(np.abs(r - 160) - 1.2), col, 0.5))
    return lay


def deco_vinyl(c1, c2):
    x, y = grid(DECO)
    cx = cy = DECO / 2
    r = np.hypot(x - cx, y - cy)
    th = np.arctan2(y - cy, x - cx)
    disc = aa(r - 238) * (1 - aa(r - 166))
    grooves = 0.5 + 0.5 * np.sin(r * 2.3) * (0.6 + 0.4 * np.sin(r * 0.37 + 1.3))
    base = 0.05 + 0.07 * grooves
    sheen = (np.maximum(0, np.cos(2 * (th - 0.7))) ** 14) * 0.38 + (np.maximum(0, np.cos(2 * (th + 2.2))) ** 30) * 0.22
    shade = base + sheen * (0.55 + 0.45 * grooves)
    col = np.dstack([shade * 0.96, shade * 0.97, shade * 1.05])
    lay = paint(disc, np.clip(col, 0, 1))
    over(lay, paint(aa(np.abs(r - 238) - 1.3), np.array([0.5, 0.5, 0.58]), 0.8))
    over(lay, paint(aa(np.abs(r - 168) - 3), c1))
    over(lay, paint(aa(np.abs(r - 176) - 1), c2, 0.7))
    return with_glow(lay, 6, 0.35)


def deco_equalizer(c1, c2):
    x, y = grid(DECO)
    cx = cy = DECO / 2
    r = np.hypot(x - cx, y - cy)
    th = np.arctan2(y - cy, x - cx)
    n = 56
    step = 2 * math.pi / n
    idx = np.round(th / step)
    lateral = (th - idx * step) * r
    lengths = np.array([16 + 62 * (0.35 * hash01(i) + 0.65 * abs(math.sin(i * 0.37 + 0.6)) ** 1.4) for i in range(n)])
    length = lengths[idx.astype(int) % n]
    along = r - 172
    d = np.maximum(np.abs(lateral) - 2.9, np.maximum(-along, along - length))
    mask = aa(d)
    col = ramp([c1, c2], np.clip(along / 80, 0, 1))
    lay = paint(mask, col)
    over(lay, paint(aa(np.abs(r - 168) - 1.6), c1, 0.9))
    return with_glow(lay, 7, 0.9, 0.35)


def deco_halo(c1, c2):
    x, y = grid(DECO)
    cx, cy, rx, ry = DECO / 2, 66, 104, 26
    k = np.hypot((x - cx) / rx, (y - cy) / ry)
    d = (k - 1) * min(rx, ry)
    col = ramp([c1, c2, c1], (x - (cx - rx)) / (2 * rx))
    ring = paint(aa(np.abs(d) - 5.5), col)
    lay = with_glow(ring, 9, 1.0, 0.55)
    over(lay, paint(aa(np.abs(d + 1.5) - 1.2), np.array([1, 1, 0.95]), 0.8))
    # a soft light pooling on the head
    pool = paint(np.exp(-(((x - cx) / 150) ** 2 + ((y - 118) / 46) ** 2)) * 0.28, c1)
    out = pool
    over(out, lay)
    for sx, sy, sr in ((cx - 120, cy - 16, 9), (cx + 126, cy - 8, 7), (cx + 40, cy - 40, 6)):
        out = over(out, with_glow(paint(star4(x, y, sx, sy, sr * 1.6), np.array([1, 1, 1.0])), 3, 1.0))
    return out


def deco_headphones(c1, c2):
    x, y = grid(DECO)
    cx = cy = DECO / 2
    r = np.hypot(x - cx, y - cy)
    th = np.arctan2(y - cy, x - cx)
    # headband: an arc over the top, metal-looking with a soft highlight
    ang = (th + math.pi) % (2 * math.pi) - math.pi  # -pi..pi, top of the picture is -pi/2
    inside_top = smooth(-0.1, 0.0, -(np.abs(ang + math.pi / 2) - 1.38))
    band = aa(np.abs(r - 204) - 11) * inside_top
    lit = 0.5 + 0.5 * np.cos((ang + math.pi / 2) * 2.2 - 0.6)
    bcol = ramp(["#15151c", "#3a3a48", "#15151c"], (r - 193) / 22) * (0.75 + 0.35 * lit[..., None])
    lay = paint(band, np.clip(bcol, 0, 1))
    over(lay, paint(aa(np.abs(r - 209) - 1.4) * inside_top, c1, 0.95))
    # ear cups on both sides
    for side in (-1, 1):
        ex = cx + side * 198
        ey = cy + 6
        dx, dy = np.abs(x - ex), y - ey
        box = np.hypot(np.maximum(dx - 12, 0), np.maximum(np.abs(dy) - 46, 0)) - 24
        cup = paint(aa(box), ramp(["#1a1a22", "#2c2c39"], (y - (ey - 70)) / 140))
        over(lay, cup)
        rim = paint(aa(np.abs(box + 3) - 1.5), c1, 0.95)
        over(lay, rim)
        pad_x = ex - side * 24
        pad = np.hypot(np.maximum(np.abs(x - pad_x) - 4, 0), np.maximum(np.abs(dy) - 42, 0)) - 13
        over(lay, paint(aa(pad), ramp([c2, c1], (y - (ey - 60)) / 120)))
        over(lay, paint(aa(np.abs(box + 12) - 6) * (0.3), np.array([1, 1, 1.0])))
    return with_glow(lay, 8, 0.45)


def petal_polys(cx, cy, length, ang, width=0.62):
    pts = []
    for u in np.linspace(0, 1, 24):
        w = math.sin(math.pi * u ** 0.75) * width * length * (1 - 0.12 * u)
        pts.append((u * length, w))
    outline = pts + [(px, -py) for px, py in reversed(pts)]
    return [[rotate_point(cx + px, cy + py, cx, cy, ang) for px, py in outline]]


def sakura_petal_layer(size, positions, c1, c2):
    c1, c2 = rgb(c1), rgb(c2)
    layer = blank(size[0], size[1])
    x, y = grid(size[0], size[1])
    for i, (px, py, length, ang) in enumerate(positions):
        m = polygon_layer(size, petal_polys(px, py, length, ang))
        # lighter at the tip, deeper where it meets the flower
        proj = ((x - px) * math.cos(ang) + (y - py) * math.sin(ang)) / length
        col = ramp([c2, c1, "#ffffff"], np.clip(proj, 0, 1) ** 1.2 * 0.9)
        over(layer, paint(m, col))
        vein = polygon_layer(size, [[rotate_point(px + length * 0.1, py - 0.7, px, py, ang), rotate_point(px + length * 0.78, py - 0.7, px, py, ang),
                                      rotate_point(px + length * 0.78, py + 0.7, px, py, ang), rotate_point(px + length * 0.1, py + 0.7, px, py, ang)]])
        over(layer, paint(vein * m, c2, 0.35))
    return layer


def deco_sakura(c1, c2):
    cx = cy = DECO / 2
    positions = []
    for i in range(15):
        a = math.radians(95 + i * 11.5 + hash01(i) * 6)
        rad = 186 + hash01(i, 2) * 26
        positions.append((cx + rad * math.cos(a), cy + rad * math.sin(a), 30 + hash01(i, 3) * 16, a + math.pi + (hash01(i, 4) - 0.5) * 1.8))
    for i in range(5):
        a = math.radians(300 + i * 17)
        rad = 212 + hash01(i, 9) * 20
        positions.append((cx + rad * math.cos(a), cy + rad * math.sin(a), 20 + hash01(i, 5) * 8, hash01(i, 6) * 6.28))
    lay = sakura_petal_layer((DECO, DECO), positions, c1, c2)
    x, y = grid(DECO)
    r = np.hypot(x - cx, y - cy)
    th = np.arctan2(y - cy, x - cx)
    # a thin blossom branch hugging the lower left
    window = smooth(0.0, 0.5, np.cos(th - math.radians(150)) * 0.9 + 0.1)
    twig = aa(np.abs(r - 178) - 2.8) * window
    over(lay, paint(twig, rgb("#5a3a34")))
    return with_glow(lay, 5, 0.25)


def deco_stardust(c1, c2):
    x, y = grid(DECO)
    cx = cy = DECO / 2
    r = np.hypot(x - cx, y - cy)
    th = np.arctan2(y - cy, x - cx)
    t = (th / (2 * math.pi)) % 1
    lay = paint(aa(np.abs(r - 190) - 1.3) * (0.35 + 0.65 * np.abs(np.sin(th * 3))), ramp([c1, c2, c1], t), 0.8)
    lay = with_glow(lay, 4, 0.7)
    for i in range(14):
        a = i * 2.399 + 0.4
        rad = 190 + (hash01(i, 1) - 0.5) * 34
        px, py = cx + rad * math.cos(a), cy + rad * math.sin(a)
        size = 8 + hash01(i, 2) * 17 if i % 3 == 0 else 5 + hash01(i, 2) * 8
        col = np.array([1, 1, 1.0]) if i % 2 == 0 else (c1 if i % 4 == 1 else c2)
        s = paint(star4(x, y, px, py, size * 1.7), col)
        over(lay, with_glow(s, 4, 1.0, 0.4))
    for i in range(34):
        a = hash01(i, 11) * 6.283
        rad = 168 + hash01(i, 12) * 70
        px, py = cx + rad * math.cos(a), cy + rad * math.sin(a)
        over(lay, paint(aa(np.hypot(x - px, y - py) - 1.3 - hash01(i, 13)), np.array([1, 1, 1.0]), 0.35 + 0.5 * hash01(i, 14)))
    return lay


def deco_aurora(c1, c2):
    x, y = grid(DECO)
    cx = cy = DECO / 2
    r = np.hypot(x - cx, y - cy)
    th = np.arctan2(y - cy, x - cx)
    lay = blank(DECO)
    arcs = [(176, 12, 30, c1, 0.9), (190, 16, 140, c2, 0.8), (182, 10, 250, np.array([0.65, 0.5, 1.0]), 0.7)]
    for rad, wid, centre, col, amount in arcs:
        window = np.exp(-((((th - math.radians(centre) + math.pi) % (2 * math.pi)) - math.pi) / 0.95) ** 2)
        ring = paint(aa(np.abs(r - rad) - wid / 2), col, amount) * window[..., None]
        lay = np.clip(lay + glow(ring, 7, 1.2) + ring, 0, 1)
    full = paint(aa(np.abs(r - 168) - 1.5), ramp([c1, c2, c1], ((th / (2 * math.pi)) % 1)), 0.55)
    over(lay, full)
    return lay


# Which colour of a variant stands for it on the swatch (the one you actually see most).
PRO_ACCENT = {"kitty": 3, "night": 3, "wings": 3, "blossom": 0, "flame": 1, "frost": 1, "laurel": 0, "seraph": 3, "snowfall": 1, "melody": 1, "paws": 1, "tiara": 0, "eclipse": 1, "planet": 0, "anger": 0, "splash": 0}

DECORATIONS = [
    # id, name, tier, price, painter, anim, variants [(id, label, c1, c2)]
    ("neon", "Neon Pulse", "common", 500, deco_neon, "pulse", [("cyan", "Cyan", "#2de2ff", "#ff3df2"), ("lime", "Lime", "#a6ff3d", "#2de2a8"), ("sunset", "Sunset", "#ff8a3d", "#ff3d7a")]),
    ("vinyl", "Vinyl", "rare", 700, deco_vinyl, "spin", [("classic", "Classic", "#ff4d4d", "#ffd0c0"), ("mint", "Mint", "#2de2a8", "#c8fff0"), ("violet", "Violet", "#a77bff", "#e6d8ff")]),
    ("equalizer", "Equalizer", "common", 600, deco_equalizer, "none", [("green", "Green", "#1ed760", "#b9ff5a"), ("pink", "Pink", "#ff3d9a", "#ffb35a"), ("ice", "Ice", "#3de0ff", "#a58bff")]),
    ("halo", "Halo", "rare", 800, deco_halo, "float", [("gold", "Gold", "#ffcf4a", "#fff3b8"), ("rose", "Rose", "#ff7ab8", "#ffe0f0"), ("ice", "Ice", "#8fd6ff", "#e9f8ff")]),
    ("headphones", "Headphones", "rare", 900, deco_headphones, "none", [("graphite", "Graphite", "#7d7d92", "#4d4d60"), ("pink", "Pink", "#ff5fae", "#ff2d8a"), ("green", "Green", "#1ed760", "#0f9d46")]),
    ("stardust", "Stardust", "epic", 1000, deco_stardust, "spin", [("gold", "Gold", "#ffd86b", "#ff9bd0"), ("blue", "Blue", "#7fd0ff", "#b79bff"), ("mint", "Mint", "#7dffd0", "#9bd0ff")]),
    ("aurora", "Aurora", "epic", 1100, deco_aurora, "pulse", [("northern", "Northern", "#2dffb0", "#3d8bff"), ("dusk", "Dusk", "#ff5fb0", "#7d5fff"), ("ember", "Ember", "#ffb03d", "#ff3d5f")]),
    ("kitty", "Kitty", "epic", 1100, pro.deco_kitty, "none", [("peach", "Peach", "#fff1e2", "#f6c9a2", "#ffb3c8", "#ff5f8f"), ("snow", "Snow", "#ffffff", "#dfe2f2", "#ffc4d6", "#8f7bff"), ("ginger", "Ginger", "#ffc36b", "#e8893a", "#ffc4a8", "#42c6ff"), ("shadow", "Shadow", "#8c93b4", "#555c80", "#c9a0ff", "#ff5fae")]),
    ("night", "Night Sky", "epic", 1100, pro.deco_night, "none", [("midnight", "Midnight", "#4b5a9c", "#c9d3f5", "#fff2b8", "#fff3c4"), ("rose", "Rose", "#a35a8a", "#f5cfe4", "#ffe0f0", "#ffe8c4"), ("aqua", "Aqua", "#3d8ba8", "#cdeef5", "#e6fbff", "#fff4cc")]),
    ("blossom", "Blossom", "epic", 1200, pro.deco_blossom, "none", [("sakura", "Sakura", "#ffb3cf", "#d86a93", "#8a5a48", "#6cc98a"), ("plum", "Plum", "#ffc2e8", "#c05a9a", "#6a4a5a", "#7ad0a0"), ("white", "White", "#ffffff", "#c98aa6", "#8a6a58", "#7acb98")]),
    ("flame", "Flame", "epic", 1300, pro.deco_flame, "pulse", [("fire", "Fire", "#ffe36e", "#ff8a2a", "#e5321c"), ("blue", "Blue", "#e0fbff", "#38b6ff", "#2a4bd8"), ("violet", "Violet", "#ffe0ff", "#c04dff", "#6a1ed0")]),
    ("frost", "Frost", "epic", 1300, pro.deco_frost, "none", [("ice", "Ice", "#f4fcff", "#9edcff", "#3f86b8"), ("mint", "Mint", "#f0fff8", "#8ef0d0", "#2f8f86"), ("amethyst", "Amethyst", "#f8f0ff", "#c9a2ff", "#6a45a8")]),
    ("laurel", "Laurel", "rare", 1000, pro.deco_laurel, "none", [("gold", "Gold", "#ffe38a", "#d9a022", "#ff4d6a", "#6a4a10"), ("silver", "Silver", "#f4f7ff", "#aab4cc", "#4da8ff", "#4a5470"), ("rose", "Rose gold", "#ffd6c8", "#e59a88", "#8f7bff", "#7a4038")]),
    ("seraph", "Seraph", "epic", 1200, pro.deco_seraph, "float", [("angel", "Angel", "#f4f7ff", "#d9e2ff", "#8c9ad0", "#bfe0ff"), ("dusk", "Dusk", "#e8d0ff", "#a98af0", "#4a2f8f", "#c7a8ff"), ("sun", "Sun", "#fff6d6", "#ffd070", "#a8741e", "#ffd45a")]),
    ("snowfall", "Snowfall", "rare", 900, pro.deco_snowfall, "none", [("ice", "Ice", "#f6fdff", "#a8e4ff", "#3f86b8"), ("rose", "Rose", "#fff2f8", "#ffc4dd", "#b8507e")]),
    ("melody", "Melody", "rare", 900, pro.deco_melody, "float", [("neon", "Neon", "#7af0ff", "#b06cff", "#2a2a8f"), ("sun", "Sun", "#fff08a", "#ff8a4d", "#8a2a1a"), ("mint", "Mint", "#b8ffd8", "#2fd69a", "#0f6a50")]),
    ("paws", "Paws", "common", 700, pro.deco_paws, "none", [("pink", "Pink", "#ffe4ee", "#ff8fb4", "#a83a63"), ("sky", "Sky", "#e6f6ff", "#7ac8ff", "#2a5f9a"), ("cream", "Cream", "#fff6e6", "#f2c58a", "#8a5a2a")]),
    ("tiara", "Tiara", "epic", 1200, pro.deco_tiara, "none", [("gold", "Gold", "#ffe38a", "#d9a022", "#ff4d6a", "#6a4a10"), ("silver", "Silver", "#f4f7ff", "#aab4cc", "#4da8ff", "#4a5470"), ("rose", "Rose gold", "#ffd6c8", "#e59a88", "#8f7bff", "#7a4038")]),
    ("eclipse", "Eclipse", "epic", 1300, ideas.deco_eclipse, "pulse", [("ember", "Ember", "#ffb347", "#ff6a1a", "#7a3a22", "#2a1410"), ("violet", "Violet", "#d9b8ff", "#9a5cff", "#4a3a6a", "#1a1030"), ("ice", "Ice", "#cdf3ff", "#5ac8ff", "#3a5a78", "#101a2a")]),
    ("planet", "Planet", "epic", 1200, ideas.deco_planet, "none", [("pink", "Pink", "#ff8fc4", "#ffd6ec", "#e8d8ff", "#fff2b8"), ("blue", "Blue", "#6ab4ff", "#cfe8ff", "#d8ecff", "#e6fbff"), ("mint", "Mint", "#4fd6a8", "#d6fff0", "#e0fff4", "#fff2b8")]),
    ("anger", "Fury", "rare", 1000, ideas.deco_anger, "pulse", [("red", "Red", "#ff4040", "#8a1020", "#ffe27a", "#ffffff"), ("blue", "Blue", "#4a8aff", "#122a8a", "#bfe0ff", "#ffffff"), ("violet", "Violet", "#b05aff", "#401a8a", "#ffd6ff", "#ffffff")]),
    ("splash", "Splash", "rare", 1000, ideas.deco_splash, "none", [("aqua", "Aqua", "#3fb8ff", "#e8fbff", "#145a9a"), ("rose", "Rose", "#ff7ab8", "#fff0f8", "#8a1f5a"), ("lime", "Lime", "#7ae84a", "#f2ffe0", "#2a6a14")]),
]

# Only the decorations that look good enough to sell. The rest stay in the file as drafts; add an id here to ship it.
SHIPPED = set()
DECORATIONS = [d for d in DECORATIONS if d[0] in SHIPPED]


# ---------------------------------------------------------------- decorations drawn by an image model, cut out and recoloured

# id, name, tier, price, how the whole picture moves, layers that move on their own (or None), sparkles over it, avatar fraction (None = standard),
# variants (id, label, swatch colour, picture file or None, hue turn in degrees)
def layer(x, y, rotate=0.0, period=3.0, phase=0.0, pivot=(0.5, 0.5), bob=0.0, bob_period=2.6, sign=1):
    return {"x": list(x), "y": list(y), "rotate": rotate, "period": period, "phase": phase, "pivot": list(pivot), "bob": bob, "bob_period": bob_period, "sign": sign}


AI_ART = [
    ("kitty", "Kitty", "epic", 1100, "none",
     [layer((0, 1), (0, 0.5), rotate=1.5, period=3.6, pivot=(0.5, 0.5)), layer((0, 1), (0.5, 1), bob=0.012, bob_period=2.4)],
     {"sprites": ["star"], "count": 8, "size": [0.02, 0.04], "twinkle": [1.6, 3.2], "blend": "add", "tint": True}, None,
     [("gray", "Grey", "#b8bccc", "kitty_gray", 0), ("white", "Snow", "#ffffff", "kitty_white", 0), ("ginger", "Ginger", "#f0a24a", "kitty_ginger", 0), ("black", "Shadow", "#8a7aa8", "kitty_black", 0)]),
    ("night", "Night Sky", "epic", 1100, "float", None,
     {"sprites": ["star"], "count": 22, "size": [0.02, 0.06], "twinkle": [1.2, 3.0], "blend": "add", "tint": True}, None,
     [("gold", "Gold", "#ffd86b", "night_gold", 0), ("aqua", "Aqua", "#8fe0ff", "night_gold", 150), ("rose", "Rose", "#ffa8d0", "night_gold", 285)]),
    ("moon", "Crescent", "epic", 1100, "pulse", None,
     {"sprites": ["star"], "count": 20, "size": [0.02, 0.055], "twinkle": [1.4, 3.4], "blend": "add", "tint": True}, None,
     [("pink", "Pink", "#ffc2dd", "moon_pink", 0), ("blue", "Blue", "#a8dcff", "moon_blue", 0), ("mint", "Mint", "#b8ffd8", "moon_blue", -60)]),
    ("blossom", "Blossom", "epic", 1200, "float", None,
     {"sprites": ["petal"], "count": 14, "size": [0.03, 0.06], "rise": -0.3, "speed": 0.14, "blend": "normal", "tint": False}, None,
     [("pink", "Pink", "#ffb3cf", "blossom_pink", 0), ("plum", "Plum", "#e0508a", "blossom_plum", 0), ("cream", "Cream", "#fff3d0", "blossom_cream", 0)]),
    ("seraph", "Seraph", "epic", 1300, "none",
     [layer((0, 0.5), (0, 1), rotate=3.6, period=2.8, phase=0.0, pivot=(0.4, 0.52), bob=0.008), layer((0.5, 1), (0, 1), rotate=3.6, period=2.8, phase=0.0, pivot=(0.6, 0.52), bob=0.008, sign=-1)],
     {"sprites": ["star"], "count": 10, "size": [0.02, 0.045], "twinkle": [1.6, 3.4], "blend": "add", "tint": True}, 0.525,
     [("white", "Angel", "#e8f0ff", "wings_white", 0), ("rose", "Rose", "#e0a8ff", "wings_rose", 0), ("gold", "Gold", "#ffd98a", "wings_gold", 0)]),
    ("eclipse", "Eclipse", "epic", 1300, "pulse", None,
     {"sprites": ["glow"], "count": 22, "size": [0.02, 0.05], "rise": 0.24, "speed": 0.22, "blend": "add", "tint": True}, None,
     [("ember", "Ember", "#ff9a3a", "eclipse", 0), ("violet", "Violet", "#b48cff", "eclipse", 235), ("ice", "Ice", "#7fd8ff", "eclipse", 165)]),
    ("planet", "Planet", "epic", 1200, "float", None,
     {"sprites": ["star"], "count": 24, "size": [0.02, 0.055], "twinkle": [1.2, 3.2], "blend": "add", "tint": True}, None,
     [("pink", "Pink", "#ffe08a", "planet", 0), ("blue", "Blue", "#a8dcff", "planet", -70), ("mint", "Mint", "#b8ffd8", "planet", -150)]),
    ("fury", "Fury", "rare", 1000, "pulse", None,
     {"sprites": ["star"], "count": 14, "size": [0.02, 0.05], "twinkle": [2.0, 4.0], "blend": "add", "tint": True}, None,
     [("red", "Red", "#ff6a5a", "fury", 0), ("blue", "Blue", "#6aa8ff", "fury", 215), ("violet", "Violet", "#c28cff", "fury", 275)]),
    ("splash", "Splash", "rare", 1000, "float", None,
     {"sprites": ["flake"], "count": 22, "size": [0.015, 0.04], "rise": 0.22, "speed": 0.16, "blend": "normal", "tint": True}, None,
     [("aqua", "Aqua", "#8fe0ff", "splash", 0), ("rose", "Rose", "#ffa8d0", "splash", 125), ("lime", "Lime", "#c8ff9a", "splash", -105)]),
]


def hue_turn(image, degrees):
    if degrees == 0:
        return image
    rgb_part = image.convert("RGB").convert("HSV")
    arr = np.asarray(rgb_part).copy()
    arr[..., 0] = (arr[..., 0].astype(int) + int(degrees * 255 / 360)) % 256
    turned = Image.fromarray(arr, "HSV").convert("RGB")
    turned.putalpha(image.getchannel("A"))
    return turned


# ---------------------------------------------------------------- nameplates (800 x 200, text sits on the left)

NP_W, NP_H = 800, 200


def np_base(stops, extra=None):
    x, y = grid(NP_W, NP_H)
    t = x / NP_W
    col = ramp(stops, t)
    shade = 1 - 0.18 * (y / NP_H)[..., None]
    return x, y, col * shade


def np_finish(lay):
    x, y = grid(NP_W, NP_H)
    box = np.hypot(np.maximum(np.abs(x - NP_W / 2) - (NP_W / 2 - 28), 0), np.maximum(np.abs(y - NP_H / 2) - (NP_H / 2 - 28), 0)) - 28
    mask = aa(box)
    lay = lay * mask[..., None]
    rim = paint(aa(np.abs(box + 1.5) - 1.0), np.array([1, 1, 1.0]), 0.22)
    over(lay, rim)
    return lay


def plate_soundwave(c1, c2):
    x, y, col = np_base(["#0c0720", "#241049", "#3b1672"])
    lay = paint(np.ones_like(x), col)
    cy = NP_H / 2
    layer = blank(NP_W, NP_H)
    for i in range(46):
        bx = 330 + i * 9.6
        env = math.sin((i / 45) * math.pi) ** 0.9
        h = (10 + 78 * env * (0.35 + 0.65 * abs(math.sin(i * 0.9) * math.cos(i * 0.31 + 0.5)))) / 2
        bar = np.hypot(np.maximum(np.abs(x - bx) - 1.6, 0), np.maximum(np.abs(y - cy) - h, 0)) - 2.2
        tcol = ramp([c1, c2], np.full_like(x, i / 45))
        over(layer, paint(aa(bar), tcol))
    over(lay, with_glow(layer, 7, 0.9))
    left = smooth(0.0, 0.58, x / NP_W)
    return np_finish(lay * (0.55 + 0.45 * left)[..., None])


def plate_grooves(c1, c2):
    x, y, col = np_base(["#16110f", "#241915", "#3a2517"])
    lay = paint(np.ones_like(x), col)
    cx, cy = 690, 100
    r = np.hypot(x - cx, y - cy)
    th = np.arctan2(y - cy, x - cx)
    grooves = (0.5 + 0.5 * np.sin(r * 2.4)) * smooth(300, 100, r)
    sheen = (np.maximum(0, np.cos(2 * (th - 0.9))) ** 12) * smooth(300, 90, r) * 0.5
    over(lay, paint(np.ones_like(x), np.dstack([grooves * 0.15 + sheen] * 3), 1.0) * 0.55)
    over(lay, paint(aa(r - 52) * 1.0, c1, 0.95))
    over(lay, paint(aa(np.abs(r - 58) - 1.2), c2, 0.9))
    over(lay, paint(aa(r - 9), rgb("#15100e")))
    over(lay, paint(aa(np.abs(r - 150) - 0.9), c2, 0.35))
    left = smooth(0.0, 0.6, x / NP_W)
    return np_finish(lay * (0.5 + 0.5 * left)[..., None])


def plate_starfield(c1, c2):
    x, y, col = np_base(["#05061a", "#0b1040", "#151a5c"])
    lay = paint(np.ones_like(x), col)
    field = gaussian_filter(rng.random((NP_H, NP_W)), 26)
    field = (field - field.min()) / (field.max() - field.min())
    field2 = gaussian_filter(rng.random((NP_H, NP_W)), 38)
    field2 = (field2 - field2.min()) / (field2.max() - field2.min())
    over(lay, paint(smooth(0.45, 0.95, field) * smooth(0.25, 1.0, x / NP_W), c1, 0.5))
    over(lay, paint(smooth(0.5, 0.95, field2) * smooth(0.4, 1.0, x / NP_W), c2, 0.45))
    stars = blank(NP_W, NP_H)
    for i in range(110):
        px, py = rng.random() * NP_W, rng.random() * NP_H
        a = 0.25 + 0.75 * rng.random() ** 2
        over(stars, paint(aa(np.hypot(x - px, y - py) - 0.6 - 0.9 * rng.random()), np.array([1, 1, 1.0]), a))
    for px, py, s in ((560, 54, 16), (700, 142, 12), (640, 96, 8), (760, 40, 10)):
        over(stars, with_glow(paint(star4(x, y, px, py, s * 1.7), np.array([1, 1, 1.0])), 4, 1.0, 0.4))
    over(lay, stars)
    left = smooth(0.0, 0.55, x / NP_W)
    return np_finish(lay * (0.6 + 0.4 * left)[..., None])


def plate_sunset(c1, c2):
    x, y = grid(NP_W, NP_H)
    t = y / NP_H
    col = ramp(["#2a0a45", c1, c2], t * 0.85 + (x / NP_W) * 0.2)
    lay = paint(np.ones_like(x), col)
    cx, cy, R = 640, 118, 84
    r = np.hypot(x - cx, y - cy)
    stripes = np.where(y > cy - 6, (np.sin((y - cy) * 0.55) > -0.15 + (y - cy) / 160).astype(float), 1.0)
    sun = aa(r - R) * stripes
    over(lay, with_glow(paint(sun, ramp(["#fff1a8", "#ff7a3d"], (y - (cy - R)) / (2 * R)) if False else ramp(["#fff4b0", "#ff9a4a", "#ff4f8b"], np.clip((y - (cy - R)) / (2 * R), 0, 1))), 18, 0.7))
    horizon = paint(aa(np.abs(y - 158) - 1.0), np.array([1, 0.8, 0.9]), 0.6) * smooth(80, 330, x)[..., None]
    over(lay, horizon)
    over(lay, paint(smooth(158, 200, y) * 0.55, rgb("#1a0630")))
    left = smooth(0.0, 0.6, x / NP_W)
    return np_finish(lay * (0.42 + 0.58 * left)[..., None])


def plate_sakura(c1, c2):
    x, y, col = np_base(["#2b0c24", "#5a1840", "#a8346a"])
    lay = paint(np.ones_like(x), col)
    for i in range(9):
        px, py, rr = 360 + hash01(i) * 430, hash01(i, 2) * NP_H, 16 + hash01(i, 3) * 38
        over(lay, paint(aa(np.hypot(x - px, y - py) - rr) * 0.10, c1))
    positions = [(380 + hash01(i, 5) * 400, 20 + hash01(i, 6) * 160, 16 + hash01(i, 7) * 20, hash01(i, 8) * 6.28) for i in range(13)]
    pet = sakura_petal_layer((NP_W, NP_H), positions, c1, c2)
    over(lay, pet * smooth(0.3, 0.7, x / NP_W)[..., None])
    left = smooth(0.0, 0.55, x / NP_W)
    return np_finish(lay * (0.5 + 0.5 * left)[..., None])


def plate_circuit(c1, c2):
    x, y, col = np_base(["#04100f", "#082421", "#0d3a34"])
    lay = paint(np.ones_like(x), col)
    traces = Image.new("L", (NP_W * 2, NP_H * 2), 0)
    d = ImageDraw.Draw(traces)
    nodes = []
    for row in range(8):
        px, py = 420 + (row % 3) * 40 + rng.random() * 60, 12 + row * 24 + rng.random() * 8
        pts = [(px, py)]
        for _ in range(5):
            px += 30 + rng.random() * 50
            pts.append((px, py))
            dy = (rng.random() - 0.5) * 70
            py = float(np.clip(py + dy, 12, NP_H - 12))
            pts.append((px + abs(dy) * 0.0, py))
        d.line([(a * 2, b * 2) for a, b in pts], fill=255, width=3, joint="curve")
        nodes += pts[::2]
    m = np.asarray(traces.resize((NP_W, NP_H), Image.LANCZOS), float) / 255
    tl = paint(m, ramp([c1, c2], x / NP_W))
    over(lay, with_glow(tl, 5, 0.9))
    for px, py in nodes:
        over(lay, paint(aa(np.hypot(x - px, y - py) - 3.4), np.array([1, 1, 1.0]), 0.9) * smooth(0.5, 0.65, px / NP_W))
    left = smooth(0.0, 0.58, x / NP_W)
    return np_finish(lay * (0.5 + 0.5 * left)[..., None])


NAMEPLATES = [
    ("soundwave", "Soundwave", "common", 400, plate_soundwave, [("neon", "Neon", "#2de2ff", "#ff3df2"), ("lime", "Lime", "#b6ff3d", "#2de2a8"), ("gold", "Gold", "#ffd86b", "#ff7a3d")]),
    ("grooves", "Record Grooves", "common", 450, plate_grooves, [("red", "Red", "#d92b2b", "#ffb0a0"), ("teal", "Teal", "#17b8a6", "#b8fff3"), ("gold", "Gold", "#e0a82e", "#fff0b8")]),
    ("starfield", "Starfield", "rare", 600, plate_starfield, [("violet", "Violet", "#7a3dff", "#ff3da8"), ("blue", "Blue", "#2d7dff", "#2de2ff"), ("emerald", "Emerald", "#1ed78a", "#3dc8ff")]),
    ("sunset", "Sunset Drive", "rare", 650, plate_sunset, [("miami", "Miami", "#d9358f", "#ff9a4a"), ("dusk", "Dusk", "#6a3dd9", "#ff5f8a"), ("tangerine", "Tangerine", "#d9602d", "#ffd04a")]),
    ("sakura", "Sakura Breeze", "rare", 650, plate_sakura, [("pink", "Pink", "#ffc2d9", "#ff8fb8"), ("white", "White", "#ffffff", "#f3c6d8"), ("lilac", "Lilac", "#e6ccff", "#b98cf5")]),
    ("circuit", "Circuit", "epic", 800, plate_circuit, [("green", "Green", "#1ed760", "#3dffd0"), ("amber", "Amber", "#ffb03d", "#ff5f3d"), ("cyan", "Cyan", "#3de0ff", "#7d9bff")]),
]


# ---------------------------------------------------------------- profile frames (256 x 256, nine-slice: 72 px corners)

FR = 256
EDGE = 72


def frame_canvas():
    x, y = grid(FR)
    # distance in from the outer edge, per side, and a corner coordinate system for ornaments
    inset = np.minimum(np.minimum(x, FR - x), np.minimum(y, FR - y))
    return x, y, inset


def frame_gold(c1, c2):
    x, y, inset = frame_canvas()
    hi = ramp([c2, c1, "#7a5200"], np.clip((x + y) / (2 * FR), 0, 1))
    band = aa(np.abs(inset - 11) - 4.2)
    lay = paint(band, hi)
    over(lay, paint(aa(np.abs(inset - 11) - 1.0) * 0.8, np.array([1, 0.98, 0.9])))
    over(lay, paint(aa(np.abs(inset - 21) - 1.2), hi, 0.8))
    over(lay, paint(aa(np.abs(inset - 4) - 1.0), hi, 0.7))
    # corner scrolls: two quarter-circles and a bead
    for (sx, sy, fx, fy) in ((0, 0, 1, 1), (FR, 0, -1, 1), (0, FR, 1, -1), (FR, FR, -1, -1)):
        lx, ly = (x - sx) * fx, (y - sy) * fy
        for rad, wid, shift in ((46, 3.2, 0), (32, 2.6, 0), (19, 2.2, 0)):
            rr = np.hypot(lx - 12, ly - 12)
            over(lay, paint(aa(np.abs(rr - rad) - wid / 2) * (lx > 11) * (ly > 11), hi))
        over(lay, paint(aa(np.hypot(lx - 56, ly - 56) - 7), hi))
        over(lay, paint(aa(np.hypot(lx - 56, ly - 56) - 3), np.array([1, 0.97, 0.85])))
        for t in range(3):
            over(lay, paint(aa(np.hypot(lx - (22 + t * 15), ly - 11) - 3), hi))
            over(lay, paint(aa(np.hypot(lx - 11, ly - (22 + t * 15)) - 3), hi))
    return with_glow(lay, 5, 0.35)


def frame_neon(c1, c2):
    x, y, inset = frame_canvas()
    dx, dy = np.minimum(x, FR - x), np.minimum(y, FR - y)
    col = ramp([c1, c2], (x + y) / (2 * FR))

    def outline(offset, cut):
        """Distance to a rectangle outline whose corners are cut at 45 degrees."""
        diag = (dx + dy - (cut + offset)) / math.sqrt(2)
        straight = np.minimum(dx, dy) - offset
        return np.where((dx < cut + offset) & (dy < cut + offset), np.abs(diag), np.abs(straight))

    lay = paint(aa(outline(11, 34) - 2.6), col)
    over(lay, paint(aa(outline(25, 34) - 1.1), col, 0.7))
    for (sx, sy, fx, fy) in ((0, 0, 1, 1), (FR, 0, -1, 1), (0, FR, 1, -1), (FR, FR, -1, -1)):
        lx, ly = (x - sx) * fx, (y - sy) * fy
        over(lay, paint(aa(np.hypot(lx - 20, ly - 20) - 4.5), np.array([1, 1, 1.0])))
        over(lay, paint(aa(np.abs(np.hypot(lx - 20, ly - 20) - 10) - 1.0), col, 0.9))
    return with_glow(lay, 6, 1.1, 0.55)


def frame_ice(c1, c2):
    x, y, inset = frame_canvas()
    col = ramp([c2, c1, "#ffffff"], np.clip((x - y) / FR * 0.5 + 0.5, 0, 1))
    lay = paint(aa(np.abs(inset - 9) - 2.2), col, 0.9)
    over(lay, paint(aa(np.abs(inset - 16) - 0.9), col, 0.45))
    spikes = []
    for (sx, sy, fx, fy) in ((0, 0, 1, 1), (FR, 0, -1, 1), (0, FR, 1, -1), (FR, FR, -1, -1)):
        def P(a, b):
            return (sx + a * fx, sy + b * fy)
        spikes.append((sx, sy, fx, fy, [[P(8, 8), P(52, 11), P(24, 20)], [P(8, 8), P(11, 52), P(20, 24)], [P(10, 10), P(40, 40), P(24, 14)], [P(10, 10), P(14, 24), P(40, 40)]]))
    lay2 = blank(FR)
    for sx, sy, fx, fy, polys in spikes:
        m = polygon_layer((FR, FR), polys)
        over(lay2, paint(m, ramp(["#ffffff", c2, c1], np.clip(np.hypot(x - sx, y - sy) / 110, 0, 1))))
    over(lay, lay2)
    return with_glow(lay, 4, 0.3, 0.12)


def frame_pixel(c1, c2):
    x, y, inset = frame_canvas()
    px = np.floor(x / 4)
    py = np.floor(y / 4)
    ix = np.minimum(px, 63 - px)
    iy = np.minimum(py, 63 - py)
    inset_p = np.minimum(ix, iy)
    corner = (ix < 14) & (iy < 14)
    cd = np.minimum(ix, iy)
    base = np.zeros_like(x)
    pal = [rgb(c2), rgb(c1), rgb("#ffffff"), rgb("#14122a")]
    lay = blank(FR)
    # border of three pixel-thick bands: dark outline, colour, highlight
    outline = (inset_p == 0)
    body = (inset_p >= 1) & (inset_p <= 3)
    light = (inset_p == 2)
    # stair-step corners: extra blocks growing the border into a notched shape
    notch = corner & ((ix + iy) < 11) & ((ix + iy) >= 6)
    col = np.zeros(x.shape + (3,))
    a = np.zeros_like(x)
    a[outline | body | notch] = 1
    col[body | notch] = pal[1]
    col[light & ~notch] = pal[0]
    col[outline] = pal[3]
    # corner gems
    gem = (ix >= 5) & (ix <= 6) & (iy >= 5) & (iy <= 6) & corner
    a[gem] = 1
    col[gem] = pal[2]
    col[gem & (ix == 5)] = pal[0]
    lay = np.dstack([col * a[..., None], a])
    return lay


FRAMES = [
    ("gold", "Gilded", "rare", 800, frame_gold, [("gold", "Gold", "#ffcf4a", "#fff0b0"), ("rose", "Rose gold", "#f2a38a", "#ffe3d6"), ("silver", "Silver", "#b9c2d6", "#f2f5fb")]),
    ("neon", "Neon Edge", "common", 600, frame_neon, [("cyan", "Cyan", "#2de2ff", "#ff3df2"), ("lime", "Lime", "#a6ff3d", "#2de2a8"), ("sun", "Sun", "#ffb03d", "#ff3d7a")]),
    ("ice", "Frost", "epic", 900, frame_ice, [("blue", "Blue", "#7fd0ff", "#dff4ff"), ("mint", "Mint", "#7dffd0", "#e0fff5"), ("lilac", "Lilac", "#b79bff", "#efe7ff")]),
    ("pixel", "Arcade", "common", 600, frame_pixel, [("green", "Green", "#36d66b", "#a8f5c0"), ("red", "Red", "#ff4d5e", "#ffb3bb"), ("blue", "Blue", "#4d7dff", "#b3c8ff")]),
]


# ---------------------------------------------------------------- profile effects: sprites plus how they move

SPR = 64


def spr_petal(c):
    lay = sakura_petal_layer((SPR, SPR), [(14, 32, 36, 0.0)], "#ffc2d9", "#ff7aa8")
    return lay


def spr_note():
    x, y = grid(SPR)
    head = aa((((x - 22) * math.cos(-0.45) - (y - 46) * math.sin(-0.45)) / 11) ** 2 * 0 + np.hypot(((x - 22) * math.cos(-0.45) - (y - 46) * math.sin(-0.45)) / 1.35, ((x - 22) * math.sin(-0.45) + (y - 46) * math.cos(-0.45))) - 9)
    stem = aa(np.maximum(np.abs(x - 30.5) - 2.1, np.maximum(11 - y, y - 46)))
    flag = np.zeros_like(x)
    t = np.clip((y - 11) / 22, 0, 1)
    curve = 32 + 14 * np.sin(t * 2.4) - 4 * t
    flag = aa(np.maximum(np.abs(x - curve) - 2.6 + 1.2 * t, np.maximum(10 - y, y - 33))) * (x > 30)
    m = np.clip(head + stem + flag, 0, 1)
    return paint(m, np.array([1, 1, 1.0]))


def spr_star():
    x, y = grid(SPR)
    m = star4(x, y, 32, 32, 28)
    core = aa(np.hypot(x - 32, y - 32) - 4)
    return paint(np.clip(m + core, 0, 1), np.array([1, 1, 1.0]))


def spr_glow():
    x, y = grid(SPR)
    r = np.hypot(x - 32, y - 32)
    a = np.exp(-(r / 11) ** 2)
    return paint(a, np.array([1, 1, 1.0]))


def spr_flake():
    x, y = grid(SPR)
    r = np.hypot(x - 32, y - 32)
    soft = np.exp(-(r / 8.5) ** 2.2)
    return paint(soft, np.array([1, 1, 1.0]))


EFFECTS = [
    # id, name, tier, price, sprites, emitter (units: card widths/heights; the apps scale them), variants [(id, label, tint)]
    ("sakura", "Sakura Fall", "epic", 1100, ["petal"],
     {"count": 16, "size": [0.05, 0.095], "vy": [0.07, 0.16], "sway": [0.02, 0.05], "spin": [-1.4, 1.4], "start": "top", "fade": 0.12, "blend": "normal", "tint": False},
     [("pink", "Pink", "#ffffff"), ("white", "White", "#fff2f8")]),
    ("notes", "Rising Notes", "rare", 900, ["note"],
     {"count": 12, "size": [0.04, 0.075], "vy": [-0.16, -0.07], "sway": [0.015, 0.04], "spin": [-0.5, 0.5], "start": "bottom", "fade": 0.2, "blend": "normal", "tint": True},
     [("neon", "Neon", "#52e8ff"), ("pink", "Pink", "#ff6cc0"), ("green", "Green", "#4cf08a")]),
    ("stars", "Twinkle", "rare", 900, ["star"],
     {"count": 18, "size": [0.025, 0.07], "vy": [0.0, 0.0], "sway": [0.0, 0.004], "spin": [0, 0], "start": "anywhere", "fade": 0.5, "blend": "add", "tint": True, "twinkle": [1.2, 3.0]},
     [("gold", "Gold", "#ffe08a"), ("ice", "Ice", "#a8dcff"), ("rose", "Rose", "#ffb0d8")]),
    ("embers", "Embers", "epic", 1200, ["glow"],
     {"count": 24, "size": [0.025, 0.06], "vy": [-0.14, -0.06], "sway": [0.01, 0.03], "spin": [0, 0], "start": "bottom", "fade": 0.25, "blend": "add", "tint": True},
     [("fire", "Fire", "#ff8a3d"), ("plasma", "Plasma", "#b06cff"), ("mint", "Mint", "#3dffb0")]),
    ("snow", "Snowfall", "common", 700, ["flake"],
     {"count": 26, "size": [0.014, 0.036], "vy": [0.05, 0.11], "sway": [0.01, 0.03], "spin": [0, 0], "start": "top", "fade": 0.1, "blend": "normal", "tint": True},
     [("white", "White", "#ffffff"), ("blue", "Blue", "#cfe8ff")]),
]


def sprite_for(name):
    return {"petal": lambda: spr_petal(None), "note": spr_note, "star": spr_star, "glow": spr_glow, "flake": spr_flake}[name]()


# ---------------------------------------------------------------- build

def save(layer, path):
    path.parent.mkdir(parents=True, exist_ok=True)
    to_image(layer).save(path.with_suffix(".webp"), "WEBP", quality=90, method=6, alpha_quality=95)


def build(sheet):
    catalog = {"version": 1, "avatar_fraction": AVATAR / DECO, "frame_edge": EDGE / FR, "items": []}
    previews = []
    for id_, name, tier, price, painter, anim, variants in DECORATIONS:
        vs = []
        for vid, label, *cols in variants:
            lay = painter(*[rgb(c) for c in cols])
            save(lay, OUT / "decorations" / f"{id_}_{vid}.webp")
            vs.append({"id": vid, "label": label, "color": cols[PRO_ACCENT.get(id_, 0)]})
            previews.append(("decoration", lay, f"{id_}_{vid}"))
        catalog["items"].append({"id": f"deco_{id_}", "kind": "decoration", "name": name, "tier": tier, "price": price, "anim": anim, "folder": "decorations", "stem": id_, "variants": vs})
    for id_, name, tier, price, anim, layers, particles, fraction, variants in AI_ART:
        vs = []
        for vid, label, swatch, source, turn in variants:
            picture = hue_turn(Image.open(OUT / "_src" / "ai" / f"{source}.png").convert("RGBA"), turn)
            (OUT / "decorations").mkdir(parents=True, exist_ok=True)
            picture.save(OUT / "decorations" / f"{id_}_{vid}.webp", "WEBP", quality=92, method=6, alpha_quality=95)
            vs.append({"id": vid, "label": label, "color": swatch})
            arr = np.asarray(picture, float) / 255
            arr[..., :3] *= arr[..., 3:4]
            previews.append(("decoration", arr, f"{id_}_{vid}"))
        entry = {"id": f"deco_{id_}", "kind": "decoration", "name": name, "tier": tier, "price": price, "anim": anim, "folder": "decorations", "stem": id_, "variants": vs, "particles": particles}
        if layers:
            entry["layers"] = layers
        if fraction:
            entry["fraction"] = fraction
        catalog["items"].append(entry)
    for id_, name, tier, price, painter, variants in NAMEPLATES:
        vs = []
        for vid, label, a, b in variants:
            lay = painter(rgb(a), rgb(b))
            save(lay, OUT / "nameplates" / f"{id_}_{vid}.webp")
            vs.append({"id": vid, "label": label, "color": a})
            previews.append(("nameplate", lay, f"{id_}_{vid}"))
        catalog["items"].append({"id": f"plate_{id_}", "kind": "nameplate", "name": name, "tier": tier, "price": price, "folder": "nameplates", "stem": id_, "variants": vs})
    for id_, name, tier, price, painter, variants in FRAMES:
        vs = []
        for vid, label, a, b in variants:
            lay = painter(rgb(a), rgb(b))
            save(lay, OUT / "frames" / f"{id_}_{vid}.webp")
            vs.append({"id": vid, "label": label, "color": a})
            previews.append(("frame", lay, f"{id_}_{vid}"))
        catalog["items"].append({"id": f"frame_{id_}", "kind": "frame", "name": name, "tier": tier, "price": price, "folder": "frames", "stem": id_, "variants": vs})
    sprites = {}
    for n in ("petal", "note", "star", "glow", "flake"):
        sprites[n] = sprite_for(n)
        save(sprites[n], OUT / "sprites" / f"{n}.png")
    for id_, name, tier, price, used, emitter, variants in EFFECTS:
        vs = [{"id": vid, "label": label, "color": tint} for vid, label, tint in variants]
        catalog["items"].append({"id": f"fx_{id_}", "kind": "effect", "name": name, "tier": tier, "price": price, "sprites": used, "emitter": emitter, "variants": vs})
    (OUT / "catalog.json").write_text(json.dumps(catalog, indent=1), encoding="utf-8")
    shop = [{"id": i["id"], "kind": i["kind"], "price": i["price"], "variants": [v["id"] for v in i["variants"]]} for i in catalog["items"]]
    header = "// Generated by scripts/shop_assets.py. Prices live here, on the server."
    (ROOT / "accounts" / "shop_catalog.mjs").write_text(header + chr(10) + "export const SHOP = " + json.dumps(shop) + ";" + chr(10), encoding="utf-8")
    bundle()
    if sheet:
        write_sheet(previews, catalog)
    return catalog


def bundle():
    """Copies the art where the apps read it: the Android assets folder, and a Rust table that embeds it in the PC app."""
    import shutil
    target = ROOT / "android" / "app" / "src" / "main" / "assets" / "shop"
    shutil.rmtree(target, ignore_errors=True)
    files = sorted(p for p in OUT.rglob("*") if p.is_file() and not p.name.startswith("_") and "_src" not in p.parts)
    for f in files:
        dest = target / f.relative_to(OUT)
        dest.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(f, dest)
    rows = []
    for f in files:
        rel = f.relative_to(OUT).as_posix()
        rows.append('        "%s" => Some(include_bytes!("../assets/shop/%s")),' % (rel, rel))
    code = ("//! Generated by scripts/shop_assets.py: the shop art, embedded in the app." + chr(10)
            + "pub fn bytes(path: &str) -> Option<&'static [u8]> {" + chr(10) + "    match path {" + chr(10)
            + chr(10).join(rows) + chr(10) + "        _ => None," + chr(10) + "    }" + chr(10) + "}" + chr(10))
    (ROOT / "src" / "shop_assets.rs").write_text(code, encoding="utf-8")


def write_sheet(previews, catalog):
    pad = 14
    bg = (22, 22, 28, 255)
    rows = []
    # decorations on a dummy avatar, 3 per variant row
    decos = [p for p in previews if p[0] == "decoration"]
    cell = 256
    sheet_w = cell * 6 + pad * 7
    sheet = Image.new("RGBA", (sheet_w, (cell + pad) * math.ceil(len(decos) / 6) + pad), bg)
    for i, (_, lay, name) in enumerate(decos):
        img = to_image(lay).resize((cell, cell), Image.LANCZOS)
        tile = Image.new("RGBA", (cell, cell), (30, 30, 38, 255))
        av = Image.new("RGBA", (cell, cell), (0, 0, 0, 0))
        d = ImageDraw.Draw(av)
        s = cell * AVATAR / DECO
        d.ellipse([(cell - s) / 2, (cell - s) / 2, (cell + s) / 2, (cell + s) / 2], fill=(92, 110, 170, 255))
        tile.alpha_composite(av)
        tile.alpha_composite(img)
        sheet.paste(tile, (pad + (i % 6) * (cell + pad), pad + (i // 6) * (cell + pad)))
    sheet.save(OUT / "_preview_decorations.png")
    plates = [p for p in previews if p[0] == "nameplate"]
    pw, ph = 400, 100
    s2 = Image.new("RGBA", (pw * 3 + pad * 4, (ph + pad) * math.ceil(len(plates) / 3) + pad), bg)
    for i, (_, lay, name) in enumerate(plates):
        img = to_image(lay).resize((pw, ph), Image.LANCZOS)
        d = ImageDraw.Draw(img)
        d.text((22, 38), "Minh Anh", fill=(255, 255, 255, 255))
        s2.alpha_composite(img, (pad + (i % 3) * (pw + pad), pad + (i // 3) * (ph + pad)))
    s2.save(OUT / "_preview_nameplates.png")
    frames = [p for p in previews if p[0] == "frame"]
    fw = 256
    s3 = Image.new("RGBA", (fw * 6 + pad * 7, (fw + pad) * math.ceil(len(frames) / 6) + pad), bg)
    for i, (_, lay, name) in enumerate(frames):
        s3.alpha_composite(to_image(lay), (pad + (i % 6) * (fw + pad), pad + (i // 6) * (fw + pad)))
    s3.save(OUT / "_preview_frames.png")
    s4 = Image.new("RGBA", (SPR * 5 * 2 + pad * 6, SPR * 2 + pad * 2), bg)
    for i, n in enumerate(("petal", "note", "star", "glow", "flake")):
        s4.alpha_composite(to_image(sprite_for(n)).resize((SPR * 2, SPR * 2), Image.LANCZOS), (pad + i * (SPR * 2 + pad), pad))
    s4.save(OUT / "_preview_sprites.png")


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--sheet", action="store_true")
    args = parser.parse_args()
    result = build(args.sheet)
    print(len(result["items"]), "items")
