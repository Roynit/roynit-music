"""More original decorations, with themes in the spirit of eclipses, planets, anger marks and splashes."""
import math

import numpy as np

from shop_kit import aa, blank, grid, hash01, over, paint, ramp, rgb, rotate_point, star4, with_glow
from shop_art_pro import C, N, chaikin, cloud_polys, ellipse_poly, flame_poly, mask_of, put, shape


def radial_ball(cx, cy, r, base, light=(-0.5, -0.6), dark_scale=0.45, hi="#ffffff"):
    """A lit sphere, shaded from a light at the top left, with a soft highlight."""
    x, y = grid(N)
    dx, dy = (x - cx) / r, (y - cy) / r
    d2 = dx * dx + dy * dy
    inside = aa((np.sqrt(d2) - 1) * r)
    z = np.sqrt(np.clip(1 - d2, 0, 1))
    lam = np.clip((-dx * light[0] - dy * light[1]) * 0.55 + z * 0.62, 0, 1)
    col = rgb(base)[None, None, :] * (dark_scale + (1 - dark_scale) * lam[..., None])
    spec = np.exp(-(((dx + 0.35) / 0.2) ** 2 + ((dy + 0.4) / 0.14) ** 2)) * 0.55
    col = col + spec[..., None] * (rgb(hi) - col)
    return paint(inside, np.clip(col, 0, 1))


def tapered_arc(a0, a1, r_mid, width, steps=60):
    """A curved stroke, thickest in the middle, tapering to a point at both ends."""
    outer, inner = [], []
    for k in range(steps + 1):
        t = k / steps
        a = math.radians(a0 + (a1 - a0) * t)
        w = width * math.sin(math.pi * t) ** 0.8 / 2
        outer.append((C + (r_mid + w) * math.cos(a), C + (r_mid + w) * math.sin(a)))
        inner.append((C + (r_mid - w) * math.cos(a), C + (r_mid - w) * math.sin(a)))
    return outer + inner[::-1]


def deco_eclipse(corona, glowc, cloud_top, cloud_bottom):
    out = blank(N)
    cx, cy = C, 84
    x, y = grid(N)
    r = np.hypot(x - cx, y - cy)
    th = np.arctan2(y - cy, x - cx)
    flares = blank(N)
    for i in range(44):
        a = 2 * math.pi * i / 44 + hash01(i, 1) * 0.05
        length = 14 + 40 * hash01(i, 2) ** 2
        half = 0.03 + 0.03 * hash01(i, 3)
        tip = (cx + (50 + length) * math.cos(a), cy + (50 + length) * math.sin(a))
        b1 = (cx + 46 * math.cos(a - half * 4), cy + 46 * math.sin(a - half * 4))
        b2 = (cx + 46 * math.cos(a + half * 4), cy + 46 * math.sin(a + half * 4))
        over(flares, paint(mask_of([[b1, tip, b2]]), ramp([corona, glowc], np.array([hash01(i, 4)]))[0], 0.9))
    put(out, with_glow(flares, 9, 1.1, 0.8))
    ring = paint(aa(np.abs(r - 50) - 5.5), ramp([glowc, "#fff6c8", corona], np.cos(th - 0.8) * 0.5 + 0.5))
    put(out, with_glow(ring, 7, 1.2, 0.9))
    put(out, radial_ball(cx, cy, 45, "#2a1838", dark_scale=0.5, hi="#9a6cd0"))
    for cxx, cyy, sc in ((C - 30, 172, 0.8), (96, 168, 0.6), (428, 170, 0.62), (C + 96, 184, 0.5)):
        put(out, shape(mask_of(cloud_polys(cxx, cyy, sc)), cloud_top, cloud_bottom, outline="#3a1d10", ow=2.4, depth=15 * sc, shade=0.4, rim=0.7, light=(0.3, -0.9)))
    return with_glow(out, 4, 0.12)


def deco_planet(base, ring_c, cloud, star):
    out = blank(N)
    px, py, pr = 398, 104, 52
    xs, ys = grid(N)
    tilt = math.radians(-22)
    cs, sn = math.cos(tilt), math.sin(tilt)
    u = (xs - px) * cs + (ys - py) * sn
    v = -(xs - px) * sn + (ys - py) * cs
    ring_m = aa((np.hypot(u / 96, v / 24) - 1) * 24) * (1 - aa((np.hypot(u / 66, v / 16) - 1) * 16))
    ring_col = ramp([ring_c, "#ffffff", ring_c], np.clip((u + 96) / 192, 0, 1))
    put(out, paint(ring_m * (v < 0), ring_col * 0.8))
    put(out, radial_ball(px, py, pr, base, dark_scale=0.4))
    bands = (np.hypot(xs - px, ys - py) < pr) * (0.12 + 0.1 * np.sin((ys - py) * 0.32))
    put(out, paint(bands, rgb("#ffffff"), 0.5))
    put(out, paint(ring_m * (v >= 0), ring_col))
    put(out, radial_ball(448, 170, 14, "#d8c0ff", dark_scale=0.4))
    for cxx, cyy, sc in ((112, 404, 1.0), (404, 428, 0.7), (60, 150, 0.5)):
        put(out, shape(mask_of(cloud_polys(cxx, cyy, sc)), "#ffffff", cloud, outline="#8a6ab8", ow=2.2, depth=14 * sc, shade=0.38, rim=0.6, spec=(cxx - 20 * sc, cyy - 22 * sc, 30 * sc, 12 * sc)))
    stars = blank(N)
    for i in range(18):
        a = hash01(i, 51) * 6.283
        rad = 178 + hash01(i, 52) * 70
        sx, sy = C + rad * math.cos(a), C + rad * math.sin(a)
        if (sx - px) ** 2 + (sy - py) ** 2 < 100 ** 2:
            continue
        over(stars, with_glow(paint(star4(xs, ys, sx, sy, 5 + hash01(i, 53) * 10), rgb(star)), 3, 1.0, 0.4))
    over(stars, out)
    return with_glow(stars, 4, 0.1)


def deco_anger(red, deep, spark, white):
    out = blank(N)
    for a0, a1, rad, w in ((196, 338, 182, 20), (16, 158, 182, 20), (212, 322, 204, 9), (30, 150, 204, 9)):
        put(out, with_glow(shape(mask_of([tapered_arc(a0, a1, rad, w)]), red, deep, outline="#4a0a12", ow=1.8, depth=6, shade=0.3, rim=0.6), 6, 0.6, 0.4))
    mx, my = 404, 100
    for k in range(4):
        a = math.radians(45 + 90 * k)
        side = []
        for t in np.linspace(0, 1, 22):
            rr = 10 + 38 * t
            wid = 15 * math.sin(math.pi * (0.15 + 0.85 * t)) ** 0.9 * (1 - 0.5 * t)
            side.append((rr, wid))
        poly = [(mx + rr * math.cos(a) - wid * math.sin(a), my + rr * math.sin(a) + wid * math.cos(a)) for rr, wid in side]
        poly += [(mx + rr * math.cos(a) + wid * math.sin(a), my + rr * math.sin(a) - wid * math.cos(a)) for rr, wid in side[::-1]]
        put(out, with_glow(shape(mask_of([chaikin(poly, 2)]), "#ff8a8a", red, outline="#4a0a12", ow=2.0, depth=7, shade=0.3, rim=0.7), 5, 0.5))
    bx, by = 112, 408
    put(out, shape(mask_of([ellipse_poly(bx, by, 30, 30)]), white, "#d0d0dc", outline="#3a3a48", ow=2.4, depth=8, shade=0.25, rim=0.6))
    bar = chaikin([(bx - 5, by - 18), (bx + 5, by - 18), (bx + 3, by + 3), (bx - 3, by + 3)], 2)
    put(out, shape(mask_of([bar]), red, deep, outline="#3a0a12", ow=1.0, depth=3, shade=0.2))
    put(out, shape(mask_of([ellipse_poly(bx, by + 12, 4.4, 4.4)]), red, deep, outline="#3a0a12", ow=1.0, depth=3, shade=0.2))
    xs, ys = grid(N)
    for sx, sy, s in ((60, 120, 9), (448, 300, 11), (440, 410, 8), (66, 300, 8), (300, 456, 9)):
        over(out, with_glow(paint(star4(xs, ys, sx, sy, s * 1.6), rgb(spark)), 4, 1.0, 0.5))
    return out


def deco_splash(water, foam, deep):
    out = blank(N)
    crowns = []
    for i in range(9):
        a = math.radians(212 + i * 12.8 + (hash01(i, 5) - 0.5) * 4)
        length = 46 + 40 * math.sin(math.pi * (i + 0.5) / 9) + 12 * hash01(i, 6)
        width = 17 + 8 * hash01(i, 7)
        lean = (4 - i) * 0.07 + (hash01(i, 8) - 0.5) * 0.25
        crowns.append((a, length, width, lean))
    crowns.sort(key=lambda t: -t[1])
    for a, length, width, lean in crowns:
        bx, by = C + 158 * math.cos(a), C + 158 * math.sin(a)
        poly = flame_poly(bx, by, length, width, a, lean)
        put(out, shape(mask_of([poly]), foam, water, outline=deep, ow=2.0, depth=12, shade=0.32, rim=0.7, spec=(bx + length * 0.5 * math.cos(a) - 6, by + length * 0.5 * math.sin(a) - 6, 8, 18)))
    for i in range(14):
        a = math.radians(205 + hash01(i, 11) * 130)
        rad = 232 + hash01(i, 12) * 22
        dx, dy = C + rad * math.cos(a), C + rad * math.sin(a) - 6
        r = 4 + hash01(i, 13) * 9
        put(out, shape(mask_of([ellipse_poly(dx, dy, r, r * 1.12)]), foam, water, outline=deep, ow=1.4, depth=r * 0.8, shade=0.3, rim=0.7, spec=(dx - r * 0.3, dy - r * 0.4, r * 0.3, r * 0.25)))
    for i in range(7):
        a = math.radians(20 + i * 24)
        bx, by = C + 196 * math.cos(a), C + 196 * math.sin(a)
        r = 6 + 6 * hash01(i, 21)
        ring = mask_of([ellipse_poly(bx, by, r, r)]) * (1 - mask_of([ellipse_poly(bx, by, r - 2.2, r - 2.2)]))
        put(out, paint(ring, rgb(foam), 0.85))
        put(out, paint(mask_of([ellipse_poly(bx - r * 0.3, by - r * 0.3, r * 0.2, r * 0.2)]), rgb("#ffffff"), 0.9))
    return with_glow(out, 5, 0.18)
