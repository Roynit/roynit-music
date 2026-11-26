"""Turns game-icons.net silhouettes (CC BY 3.0, see THIRD_PARTY_NOTICES.md) into masks the shop painters can shade.

The SVGs are one white path on a black square. The path is flattened to polygons and filled with the even-odd rule,
so holes stay holes.
"""
from pathlib import Path

import numpy as np
from PIL import Image, ImageDraw
from svgelements import Path as SvgPath

SRC = Path(__file__).resolve().parent.parent / "assets" / "shop" / "_src" / "game-icons"
_cache = {}


def icon_mask(name, size=512, ss=3):
    """A 0..1 mask of an icon filling a size x size square (name like "lorc/angel-wings")."""
    key = (name, size)
    if key in _cache:
        return _cache[key]
    text = (SRC / (name + ".svg")).read_text(encoding="utf-8")
    import re
    paths = re.findall(r'<path[^>]*?d="([^"]+)"', text)
    d = paths[-1]  # the first path is the black background square
    path = SvgPath(d)
    big = size * ss
    scale = big / 512.0
    # even-odd fill: draw each sub-path on its own layer and XOR them together
    acc = np.zeros((big, big), bool)
    for sub in path.as_subpaths():
        sp = SvgPath(sub)
        n = int(max(200, min(8000, sp.length() * scale / 1.5)))
        pts = []
        for t in np.linspace(0, 1, n):
            p = sp.point(t)
            pts.append((p.x * scale, p.y * scale))
        layer = Image.new("1", (big, big), 0)
        ImageDraw.Draw(layer).polygon(pts, fill=1)
        acc ^= np.asarray(layer, bool)
    m = Image.fromarray((acc * 255).astype(np.uint8)).resize((size, size), Image.LANCZOS)
    out = np.asarray(m, float) / 255
    _cache[key] = out
    return out
