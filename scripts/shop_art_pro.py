"""Illustrated avatar decorations: soft shaded shapes with outlines and rim light, like stickers, drawn from bezier outlines.

Each painter takes two or three colours and returns a 512 x 512 premultiplied layer with the avatar circle (radius 160)
left clear in the middle.
"""
import math

import numpy as np
from PIL import Image, ImageDraw
from scipy.ndimage import distance_transform_edt, gaussian_filter

from shop_kit import aa, blank, glow, grid, hash01, over, paint, ramp, rgb, rotate_point, smooth, star4, with_glow

N = 512
C = N / 2
SS = 4


# ------------------------------------------------------------------ shapes

def chaikin(points, rounds=3, closed=True):
    """Rounds the corners of a polygon."""
    pts = [tuple(p) for p in points]
    for _ in range(rounds):
        out = []
        n = len(pts)
        for i in range(n if closed else n - 1):
            a, b = pts[i], pts[(i + 1) % n]
            out.append((0.75 * a[0] + 0.25 * b[0], 0.75 * a[1] + 0.25 * b[1]))
            out.append((0.25 * a[0] + 0.75 * b[0], 0.25 * a[1] + 0.75 * b[1]))
        pts = out if closed else [pts[0]] + out + [pts[-1]]
    return pts


def bezier(p0, p1, p2, p3, n=28):
    t = np.linspace(0, 1, n)[:, None]
    p0, p1, p2, p3 = map(np.array, (p0, p1, p2, p3))
    return [tuple(p) for p in ((1 - t) ** 3 * p0 + 3 * (1 - t) ** 2 * t * p1 + 3 * (1 - t) * t ** 2 * p2 + t ** 3 * p3)]


def mask_of(polys, size=N):
    img = Image.new("L", (size * SS, size * SS), 0)
    d = ImageDraw.Draw(img)
    for poly in polys:
        d.polygon([(x * SS, y * SS) for x, y in poly], fill=255)
    return np.asarray(img.resize((size, size), Image.LANCZOS), float) / 255


def ellipse_poly(cx, cy, rx, ry, angle=0.0, n=48):
    pts = []
    for i in range(n):
        t = 2 * math.pi * i / n
        x, y = cx + rx * math.cos(t), cy + ry * math.sin(t)
        pts.append(rotate_point(x, y, cx, cy, angle))
    return pts


def line_poly(points, width, closed=False):
    """A thick line as one polygon (round joints by drawing it at high resolution)."""
    img = Image.new("L", (N * SS, N * SS), 0)
    d = ImageDraw.Draw(img)
    d.line([(x * SS, y * SS) for x, y in points], fill=255, width=int(width * SS), joint="curve")
    for x, y in (points[0], points[-1]):
        r = width * SS / 2
        d.ellipse([x * SS - r, y * SS - r, x * SS + r, y * SS + r], fill=255)
    return np.asarray(img.resize((N, N), Image.LANCZOS), float) / 255


def shape(mask, top, bottom=None, outline="#3a2a4a", ow=2.6, depth=9.0, shade=0.32, rim=0.5, spec=None, light=(-0.55, -0.83)):
    """A sticker-like fill: vertical gradient, darker toward the edge, rim light from the top left, dark outline."""
    top = rgb(top)
    bottom = top if bottom is None else rgb(bottom)
    inside = mask > 0.5
    if not inside.any():
        return blank(N)
    d = distance_transform_edt(inside)
    d_out = distance_transform_edt(~inside)
    ys = np.where(inside.any(axis=1))[0]
    y0, y1 = ys[0], ys[-1]
    y, _ = np.mgrid[0:N, 0:N]
    t = np.clip((y - y0) / max(y1 - y0, 1), 0, 1)[..., None]
    col = top * (1 - t) + bottom * t
    dep = gaussian_filter(np.minimum(d, depth * 2), 1.2)
    col = col * (1 - shade * (1 - smooth(0, depth, dep))[..., None])
    gy, gx = np.gradient(dep)
    lit = np.clip(-(gx * light[0] + gy * light[1]), 0, 1)
    edge = 1 - smooth(0, depth * 0.8, dep)
    col = col + (lit * edge * rim)[..., None] * (1 - col)
    if spec is not None:
        cx, cy, rx, ry = spec
        yy, xx = np.mgrid[0:N, 0:N]
        col = col + (np.exp(-(((xx - cx) / rx) ** 2 + ((yy - cy) / ry) ** 2)) * 0.45)[..., None] * (1 - col)
    fill = paint(mask, np.clip(col, 0, 1))
    if ow <= 0:
        return fill
    ring_a = smooth(ow + 0.7, ow - 0.3, d_out) * (1 - mask)
    layer = paint(ring_a, rgb(outline))
    return over(layer, fill)


def put(base, layer):
    return over(base, layer)


def ring_pos(angle_deg, radius):
    a = math.radians(angle_deg)
    return C + radius * math.cos(a), C + radius * math.sin(a)


# ------------------------------------------------------------------ cat ears, bow and paws

def deco_kitty(fur, fur2, inner, ribbon):
    out = blank(N)
    # paws first so the ears and bow sit over them
    for sx in (-1, 1):
        px = C + sx * 78
        palm = mask_of([ellipse_poly(px, 446, 46, 36)])
        finger_polys = [ellipse_poly(px + sx * 0 + dx, 414 + dy, 17, 20, 0.0) for dx, dy in ((-27, 6), (0, -2), (27, 6))]
        paw = mask_of([ellipse_poly(px, 446, 46, 36)] + finger_polys)
        put(out, shape(paw, fur, fur2, outline="#7a4f45", ow=2.6, depth=7, shade=0.25, spec=(px - 14, 428, 22, 12)))
        for dx, dy in ((-27, 5), (0, -3), (27, 5)):
            put(out, shape(mask_of([ellipse_poly(px + dx, 416 + dy, 6, 8)]), inner, inner, outline="#c4607f", ow=0.0, depth=3, shade=0.1))
        put(out, shape(mask_of([ellipse_poly(px, 452, 15, 12)] + [ellipse_poly(px + dx, 430, 5.5, 7) for dx in (-17, 17)] + [ellipse_poly(px, 424, 5.5, 7)]), inner, inner, outline="#c4607f", ow=1.4, depth=4, shade=0.12))
    # ears
    for sx in (-1, 1):
        def X(x):
            return C + sx * (x - C) * -1 if False else (x if sx == -1 else N - x)
        outer = chaikin([(X(104), 200), (X(240), 104), (X(124), 6)], rounds=4)
        put(out, shape(mask_of([outer]), fur, fur2, outline="#7a4f45", ow=3.0, depth=11, shade=0.28, spec=(X(142), 70, 18, 30)))
        inner_ear = chaikin([(X(132), 170), (X(212), 118), (X(134), 46)], rounds=4)
        put(out, shape(mask_of([inner_ear]), inner, ramp([inner, "#ff7fa6"], np.array([0.7]))[0], outline="#d46a8c", ow=1.4, depth=7, shade=0.3))
        for k in range(3):
            tuft = line_poly([(X(170 - k * 6), 134 + k * 16), (X(196 - k * 2), 116 + k * 14)], 2.0)
            put(out, paint(tuft, rgb("#fff4f6"), 0.8))
    # bow on the left ear
    bx, by = 140, 176
    loop_l = chaikin([(bx - 4, by), (bx - 52, by - 30), (bx - 58, by + 24)], rounds=4)
    loop_r = chaikin([(bx + 4, by), (bx + 52, by - 30), (bx + 58, by + 24)], rounds=4)
    for loop in (loop_l, loop_r):
        put(out, shape(mask_of([loop]), ribbon, ribbon, outline="#8d2a4a", ow=2.4, depth=8, shade=0.3, spec=(bx, by - 14, 40, 10)))
    knot = mask_of([ellipse_poly(bx, by, 13, 14)])
    put(out, shape(knot, ribbon, ribbon, outline="#8d2a4a", ow=2.4, depth=6, shade=0.25, rim=0.7))
    return with_glow(out, 5, 0.18)


# ------------------------------------------------------------------ night sky

def cloud_polys(cx, cy, scale=1.0):
    parts = [(0, 0, 38), (-40, 10, 28), (40, 12, 30), (-72, 24, 20), (72, 26, 22), (-18, -22, 26), (20, -18, 24)]
    polys = [ellipse_poly(cx + x * scale, cy + y * scale, r * scale, r * scale * 0.95) for x, y, r in parts]
    polys.append([(cx - 90 * scale, cy + 45 * scale), (cx + 94 * scale, cy + 45 * scale), (cx + 94 * scale, cy + 22 * scale), (cx - 90 * scale, cy + 22 * scale)])
    return polys


def deco_night(sky, cloud, star, moon):
    out = blank(N)
    x, y = grid(N)
    # moon, top right, with a halo
    mx, my = 416, 82
    crescent = mask_of([ellipse_poly(mx, my, 42, 42)]) * (1 - mask_of([ellipse_poly(mx + 22, my - 12, 38, 38)]))
    put(out, with_glow(paint(crescent, rgb(moon)), 14, 0.9, 0.5))
    put(out, shape(crescent, moon, "#ffd37a", outline="#a8742e", ow=1.8, depth=7, shade=0.2))
    # clouds, one bottom left and a small one top left, both drifting round the avatar
    for cx, cy, sc, tint in ((122, 392, 1.0, 0), (70, 120, 0.55, 1), (398, 424, 0.62, 1)):
        polys = cloud_polys(cx, cy, sc)
        put(out, shape(mask_of(polys), "#ffffff", cloud, outline=sky, ow=2.4, depth=14 * sc, shade=0.38, rim=0.6, spec=(cx - 20 * sc, cy - 20 * sc, 30 * sc, 14 * sc)))
    # stars
    star_layer = blank(N)
    for i in range(16):
        a = i * 2.399 + 0.3
        rad = 188 + hash01(i, 3) * 54
        px, py = C + rad * math.cos(a), C + rad * math.sin(a)
        if (px - mx) ** 2 + (py - my) ** 2 < 70 ** 2:
            continue
        size = 6 + hash01(i, 5) * 15
        over(star_layer, with_glow(paint(star4(x, y, px, py, size * 1.7), rgb(star)), 4, 1.0, 0.4))
    for i in range(38):
        a = hash01(i, 21) * 6.283
        rad = 170 + hash01(i, 22) * 78
        px, py = C + rad * math.cos(a), C + rad * math.sin(a)
        over(star_layer, paint(aa(np.hypot(x - px, y - py) - 1.0 - hash01(i, 23)), rgb(star), 0.4 + 0.5 * hash01(i, 24)))
    over(star_layer, out)
    return star_layer


# ------------------------------------------------------------------ angel wings

def feather(base, length, width, angle):
    bx, by = base
    pts = []
    for u in np.linspace(0, 1, 18):
        w = math.sin(math.pi * u ** 0.62) * width * (1 - 0.15 * u)
        pts.append((bx + u * length, by + w))
    pts2 = [(bx + u * length, by - math.sin(math.pi * u ** 0.62) * width * (1 - 0.15 * u)) for u in np.linspace(1, 0, 18)]
    return [rotate_point(px, py, bx, by, angle) for px, py in pts + pts2]


def deco_wings(top, mid, outline, glowc):
    out = blank(N)
    for sx in (-1, 1):
        shoulder = (C + sx * 166, C - 10)
        for scale, col in ((1.0, mid), (0.8, top), (0.58, "#ffffff")):
            for k, deg in enumerate(np.linspace(-74, 56, 8)):
                length = scale * (74 + 34 * math.sin(math.pi * (k + 0.8) / 9))
                width = 12 * scale + 4
                angle = math.pi - math.radians(deg) if sx == -1 else math.radians(deg)
                poly = feather(shoulder, length, width, angle)
                put(out, shape(mask_of([poly]), "#ffffff", col, outline=outline, ow=2.0, depth=8, shade=0.28, rim=0.5))
        put(out, shape(mask_of([ellipse_poly(shoulder[0], shoulder[1], 16, 22)]), "#ffffff", top, outline=outline, ow=1.8, depth=7, shade=0.2))
    return with_glow(out, 9, 0.35, 0.25)


# ------------------------------------------------------------------ cherry blossom branch

def blossom(cx, cy, r, pink, deep, rot):
    out = blank(N)
    petals = []
    for k in range(5):
        a = rot + k * 2 * math.pi / 5
        px, py = cx + r * 0.55 * math.cos(a), cy + r * 0.55 * math.sin(a)
        petals.append(ellipse_poly(px, py, r * 0.62, r * 0.46, a))
    m = mask_of(petals)
    put(out, shape(m, "#ffffff", pink, outline=deep, ow=1.6, depth=r * 0.5, shade=0.3, rim=0.5))
    for k in range(5):
        a = rot + k * 2 * math.pi / 5
        tx, ty = cx + r * 0.98 * math.cos(a), cy + r * 0.98 * math.sin(a)
        put(out, paint(mask_of([ellipse_poly(tx, ty, r * 0.1, r * 0.1)]), rgb(deep), 0.0))
        line = line_poly([(cx + r * 0.2 * math.cos(a), cy + r * 0.2 * math.sin(a)), (cx + r * 0.6 * math.cos(a), cy + r * 0.6 * math.sin(a))], 1.2)
        put(out, paint(line, rgb(deep), 0.45))
    put(out, paint(mask_of([ellipse_poly(cx, cy, r * 0.17, r * 0.17)]), rgb("#ffd45a")))
    for k in range(7):
        a = rot * 2 + k * 0.9
        put(out, paint(mask_of([ellipse_poly(cx + r * 0.34 * math.cos(a), cy + r * 0.34 * math.sin(a), 1.8, 1.8)]), rgb("#f2a03a")))
    return out


def deco_blossom(pink, deep, wood, leaf):
    out = blank(N)
    # a bough arcing along the lower left, with twigs
    arc = [ring_pos(a, 176) for a in np.linspace(95, 215, 40)]
    put(out, shape(line_poly(arc, 7), wood, "#3f2a22", outline="#2a1a14", ow=1.4, depth=3, shade=0.3, rim=0.3))
    twigs = [([ring_pos(130, 178), ring_pos(122, 206), ring_pos(110, 232)], 4.5), ([ring_pos(175, 178), ring_pos(184, 208), ring_pos(200, 228)], 4.0), ([ring_pos(205, 176), ring_pos(212, 206), ring_pos(232, 220)], 3.5)]
    for pts, w in twigs:
        put(out, shape(line_poly(pts, w), wood, "#3f2a22", outline="#2a1a14", ow=1.0, depth=2, shade=0.25, rim=0.2))
    # leaves
    for a, side in ((110, 1), (150, -1), (190, 1), (160, 1), (125, -1)):
        px, py = ring_pos(a, 182)
        ang = math.radians(a + 90 * side)
        poly = [rotate_point(px + u, py + w, px, py, ang) for u, w in [(0, 0), (10, 8), (24, 9), (36, 0), (24, -9), (10, -8)]]
        put(out, shape(mask_of([chaikin(poly, 3)]), leaf, "#2f7a4a", outline="#1e4d30", ow=1.4, depth=5, shade=0.3))
    # flowers and buds
    spots = [(100, 182, 30, 0.2), (126, 184, 26, 1.0), (146, 180, 34, 0.5), (172, 184, 27, 1.7), (200, 180, 31, 0.9), (118, 232, 22, 0.1), (206, 224, 20, 1.3), (135, 206, 17, 0.4)]
    for a, rad, size, rot in spots:
        px, py = ring_pos(a, rad) if a < 400 and size > 20 else ring_pos(a, rad)
        put(out, blossom(px, py, size, pink, deep, rot))
    for a, rad in ((188, 210), (160, 214), (112, 214)):
        px, py = ring_pos(a, rad)
        put(out, shape(mask_of([ellipse_poly(px, py, 6, 9, math.radians(a))]), "#ffd0df", deep, outline=deep, ow=1.2, depth=3, shade=0.2))
    # a few petals drifting down the other side
    for i in range(9):
        a = 330 + i * 11 + hash01(i, 8) * 8
        px, py = ring_pos(a, 196 + hash01(i, 9) * 40)
        ang = hash01(i, 10) * 6.283
        poly = [rotate_point(px + u, py + w, px, py, ang) for u, w in [(0, 0), (7, 6), (16, 5), (20, 0), (16, -5), (7, -6)]]
        put(out, shape(mask_of([chaikin(poly, 3)]), "#ffffff", pink, outline=deep, ow=1.0, depth=4, shade=0.2))
    return with_glow(out, 4, 0.12)


# ------------------------------------------------------------------ flame wreath

def flame_poly(cx, cy, length, width, angle, lean):
    pts = []
    # a teardrop whose tip curls to one side
    for u in np.linspace(0, 1, 26):
        w = math.sin(math.pi * u ** 0.7) * width * (1 - u) ** 0.3
        curl = lean * u * u * length * 0.55
        pts.append((u * length, w + curl))
    back = [(u * length, -math.sin(math.pi * u ** 0.7) * width * (1 - u) ** 0.3 + lean * u * u * length * 0.55) for u in np.linspace(1, 0, 26)]
    return [rotate_point(cx + px, cy + py, cx, cy, angle) for px, py in pts + back]


def deco_flame(core, mid, edge):
    out = blank(N)
    items = []
    count = 26
    for i in range(count):
        a = 2 * math.pi * i / count + hash01(i, 4) * 0.12
        inner_r = 156
        length = 60 + 70 * hash01(i, 5) ** 1.3 + (26 if math.sin(a) < 0 else 0)
        width = 15 + 12 * hash01(i, 6)
        lean = (hash01(i, 7) - 0.5) * 0.9
        items.append((a, inner_r, length, width, lean))
    items.sort(key=lambda t: -t[2])
    for a, inner_r, length, width, lean in items:
        cx, cy = C + inner_r * math.cos(a), C + inner_r * math.sin(a)
        poly = flame_poly(cx, cy, length, width, a, lean)
        put(out, shape(mask_of([poly]), mid, edge, outline="#6a1208", ow=1.8, depth=10, shade=0.2, rim=0.2))
        poly2 = flame_poly(cx, cy, length * 0.68, width * 0.62, a, lean)
        put(out, shape(mask_of([poly2]), core, mid, outline=mid, ow=0, depth=8, shade=0.1, rim=0.1))
        poly3 = flame_poly(cx, cy, length * 0.4, width * 0.34, a, lean * 0.6)
        put(out, paint(mask_of([poly3]), rgb("#fffbe0"), 0.9))
    return with_glow(out, 10, 0.9, 0.7)


# ------------------------------------------------------------------ frost crystals

def deco_frost(light, mid, deep):
    out = blank(N)
    shards = []
    count = 20
    for i in range(count):
        a = 2 * math.pi * i / count + hash01(i, 2) * 0.1
        length = 40 + 72 * hash01(i, 3) ** 1.4 + (28 if math.sin(a) < -0.3 else 0)
        width = 11 + 9 * hash01(i, 4)
        shards.append((a, length, width))
    shards.sort(key=lambda s: -s[1])
    for a, length, width in shards:
        bx, by = C + 160 * math.cos(a), C + 160 * math.sin(a)
        tip = (bx + (length + 20) * math.cos(a), by + (length + 20) * math.sin(a))
        mid_pt = (bx + length * 0.38 * math.cos(a), by + length * 0.38 * math.sin(a))
        nx, ny = -math.sin(a), math.cos(a)
        left = (mid_pt[0] + nx * width, mid_pt[1] + ny * width)
        right = (mid_pt[0] - nx * width, mid_pt[1] - ny * width)
        base = (bx, by)
        whole = [base, left, tip, right]
        put(out, shape(mask_of([whole]), light, mid, outline=deep, ow=1.6, depth=6, shade=0.15, rim=0.4))
        facet = mask_of([[base, left, tip]])
        put(out, paint(facet * mask_of([whole]), rgb("#ffffff"), 0.35))
        put(out, paint(mask_of([[base, right, tip]]) * mask_of([whole]), rgb(deep), 0.22))
        put(out, paint(line_poly([base, tip], 1.1), rgb("#ffffff"), 0.7))
    x, y = grid(N)
    for i in range(34):
        a = hash01(i, 31) * 6.283
        rad = 168 + hash01(i, 32) * 92
        px, py = C + rad * math.cos(a), C + rad * math.sin(a)
        if i % 5 == 0:
            over(out, with_glow(paint(star4(x, y, px, py, 6 + hash01(i, 33) * 8), rgb("#ffffff")), 3, 1.0, 0.4))
        else:
            over(out, paint(aa(np.hypot(x - px, y - py) - 1.2 - hash01(i, 34) * 1.5), rgb("#ffffff"), 0.5 + 0.4 * hash01(i, 35)))
    return with_glow(out, 8, 0.45, 0.3)


# ------------------------------------------------------------------ royal laurel and crown

def laurel_leaf(px, py, ang, length, width):
    pts = []
    for u in np.linspace(0, 1, 16):
        pts.append((u * length, math.sin(math.pi * u ** 0.8) * width))
    pts += [(u * length, -math.sin(math.pi * u ** 0.8) * width) for u in np.linspace(1, 0, 16)]
    return [rotate_point(px + a, py + b, px, py, ang) for a, b in pts]


def deco_laurel(gold, gold2, gem, deep):
    out = blank(N)
    for sx in (-1, 1):
        for i in range(15):
            u = i / 14
            a = math.radians(90 + sx * (8 + u * 128))
            rad = 176 + 8 * math.sin(u * 3)
            px, py = C + rad * math.cos(a), C + rad * math.sin(a)
            tangent = a + sx * math.pi / 2 * -1
            for side in (-1, 1):
                ang = tangent + side * 0.62 + (0 if sx == 1 else 0)
                length = 46 - 10 * u
                poly = laurel_leaf(px, py, ang if sx == 1 else ang + math.pi, length, 11 - 2 * u)
                put(out, shape(mask_of([poly]), gold, gold2, outline=deep, ow=1.6, depth=5, shade=0.28, rim=0.5))
        put(out, paint(line_poly([ring_pos(90 + sx * (8 + k * 9), 178) for k in range(15)], 3.0), rgb(deep), 0.0))
    # a small crown on top
    crown = chaikin([(196, 108), (196, 66), (222, 90), (256, 44), (290, 90), (316, 66), (316, 108)], rounds=2)
    put(out, shape(mask_of([crown]), gold, gold2, outline=deep, ow=2.4, depth=9, shade=0.3, rim=0.6, spec=(236, 70, 16, 10)))
    band = mask_of([[(194, 104), (318, 104), (318, 124), (194, 124)]])
    put(out, shape(band, gold, gold2, outline=deep, ow=2.2, depth=6, shade=0.3, rim=0.5))
    for gx, gy, r, g in ((256, 62, 8, gem), (206, 82, 5, "#ffffff"), (306, 82, 5, "#ffffff"), (256, 114, 6, gem)):
        put(out, shape(mask_of([ellipse_poly(gx, gy, r, r)]), "#ffffff", g, outline=deep, ow=1.2, depth=3, shade=0.2, spec=(gx - 2, gy - 2, 3, 3)))
    return with_glow(out, 6, 0.25)


# ------------------------------------------------------------------ decorations built from game-icons.net silhouettes (CC BY 3.0)

import shop_icons  # noqa: E402


def place_icon(name, cx, cy, size, angle=0.0, flip=False):
    """An icon's mask moved onto the 512 canvas: centred on (cx, cy), `size` px across, turned by `angle` radians."""
    m = shop_icons.icon_mask(name, int(size))
    im = Image.fromarray((m * 255).astype(np.uint8))
    if flip:
        im = im.transpose(Image.FLIP_LEFT_RIGHT)
    im = im.rotate(-math.degrees(angle), resample=Image.BICUBIC, expand=True)
    canvas = Image.new("L", (N, N), 0)
    canvas.paste(im, (int(cx - im.width / 2), int(cy - im.height / 2)), im)
    return np.asarray(canvas, float) / 255


def deco_seraph(top, mid, outline, glowc):
    out = blank(N)
    for sx in (-1, 1):
        m = place_icon("lorc/feathered-wing", C + sx * 168, C - 26, 250, angle=sx * -0.12, flip=(sx == 1))
        put(out, shape(m, "#ffffff", mid, outline=outline, ow=2.4, depth=12, shade=0.3, rim=0.6, spec=(C + sx * 190, C - 60, 36, 22)))
        m2 = place_icon("lorc/feathered-wing", C + sx * 170, C + 20, 150, angle=sx * 0.3, flip=(sx == 1))
        put(out, shape(m2, "#ffffff", top, outline=outline, ow=2.0, depth=8, shade=0.28, rim=0.5))
    return with_glow(out, 9, 0.4, 0.3)


def deco_snowfall(light, mid, deep):
    out = blank(N)
    items = []
    for i in range(18):
        a = i * 2.399 + 0.5
        rad = 182 + hash01(i, 3) * 60
        size = 34 + hash01(i, 4) * 46
        items.append((C + rad * math.cos(a), C + rad * math.sin(a), size, hash01(i, 5) * 6.28, "lorc/snowflake-1" if i % 2 else "lorc/snowflake-2"))
    items.sort(key=lambda t: -t[2])
    for x, y, size, ang, name in items:
        m = place_icon(name, x, y, size, ang)
        put(out, shape(m, light, mid, outline=deep, ow=1.6, depth=size * 0.14, shade=0.25, rim=0.55))
    xs, ys = grid(N)
    for i in range(30):
        a = hash01(i, 31) * 6.283
        rad = 170 + hash01(i, 32) * 85
        px, py = C + rad * math.cos(a), C + rad * math.sin(a)
        over(out, paint(aa(np.hypot(xs - px, ys - py) - 1.3 - hash01(i, 33) * 1.4), rgb("#ffffff"), 0.4 + 0.5 * hash01(i, 34)))
    return with_glow(out, 6, 0.4, 0.25)


def deco_melody(c1, c2, deep):
    out = blank(N)
    items = []
    for i in range(11):
        a = -1.2 + i * 0.58 + hash01(i, 6) * 0.2
        rad = 190 + hash01(i, 7) * 48
        size = 40 + hash01(i, 8) * 36
        items.append((C + rad * math.cos(a), C + rad * math.sin(a), size, (hash01(i, 9) - 0.5) * 0.9))
    for x, y, size, ang in items:
        m = place_icon("delapouite/musical-notes", x, y, size, ang)
        put(out, with_glow(shape(m, c1, c2, outline=deep, ow=1.8, depth=size * 0.13, shade=0.25, rim=0.55), 5, 0.7))
    xs, ys = grid(N)
    for i in range(10):
        a = hash01(i, 41) * 6.283
        rad = 175 + hash01(i, 42) * 70
        px, py = C + rad * math.cos(a), C + rad * math.sin(a)
        over(out, with_glow(paint(star4(xs, ys, px, py, 8 + hash01(i, 43) * 9), rgb("#ffffff")), 3, 1.0, 0.3))
    return out


def deco_paws(c1, c2, deep):
    out = blank(N)
    for i in range(10):
        a = math.radians(70 + i * 22)
        rad = 190 + 12 * math.sin(i * 1.7)
        size = 46 - abs(i - 4.5) * 2.5
        m = place_icon("lorc/paw-print", C + rad * math.cos(a), C + rad * math.sin(a), size, a + math.pi / 2 + 0.25 * (-1) ** i)
        put(out, shape(m, c1, c2, outline=deep, ow=1.8, depth=size * 0.12, shade=0.25, rim=0.55))
    return with_glow(out, 5, 0.18)


def deco_tiara(gold, gold2, gem, deep):
    out = blank(N)
    m = place_icon("delapouite/tiara", C, 74, 250)
    put(out, with_glow(shape(m, gold, gold2, outline=deep, ow=2.4, depth=10, shade=0.3, rim=0.6, spec=(C - 40, 60, 40, 14)), 6, 0.4))
    put(out, shape(mask_of([ellipse_poly(C, 38, 9, 13)]), "#ffffff", gem, outline=deep, ow=1.4, depth=4, shade=0.2, spec=(C - 2, 33, 3, 4)))
    xs, ys = grid(N)
    for i, (px, py, s) in enumerate(((84, 150, 14), (428, 150, 16), (62, 262, 10), (452, 270, 12), (110, 400, 11), (404, 410, 13), (256, 470, 12), (200, 36, 10), (312, 36, 10))):
        over(out, with_glow(paint(star4(xs, ys, px, py, s * 1.6), rgb(gold)), 4, 1.0, 0.45))
    return out
