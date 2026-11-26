"""Drawing toolkit for the shop art: premultiplied RGBA float images, signed-distance coverage, gradients, glows."""
import math
import numpy as np
from PIL import Image, ImageDraw
from scipy.ndimage import gaussian_filter

# ---------------------------------------------------------------- small toolkit (premultiplied RGBA float images)

def rgb(h):
    if not isinstance(h, str):
        return np.asarray(h, float)
    h = h.lstrip("#")
    return np.array([int(h[i:i + 2], 16) for i in (0, 2, 4)], float) / 255


def blank(w, h=None):
    return np.zeros((h or w, w, 4), float)


def grid(w, h=None):
    y, x = np.mgrid[0:(h or w), 0:w]
    return x + 0.5, y + 0.5


def aa(d):
    """Coverage of a pixel from its signed distance to the edge (negative inside), one pixel of soft edge."""
    return np.clip(0.5 - d, 0, 1)


def smooth(e0, e1, x):
    t = np.clip((x - e0) / (e1 - e0), 0, 1)
    return t * t * (3 - 2 * t)


def ramp(stops, t):
    """Colour along a list of colours for t in 0..1 (array)."""
    stops = [rgb(s) if isinstance(s, str) else s for s in stops]
    t = np.clip(t, 0, 1) * (len(stops) - 1)
    i = np.minimum(t.astype(int), len(stops) - 2)
    f = (t - i)[..., None]
    arr = np.stack(stops)
    return arr[i] * (1 - f) + arr[i + 1] * f


def paint(mask, color, alpha=1.0):
    """A layer from a coverage mask and a colour (single colour or per-pixel)."""
    a = np.clip(mask * alpha, 0, 1)
    color = np.broadcast_to(color, mask.shape + (3,))
    return np.dstack([color * a[..., None], a])


def over(dst, src):
    dst[:] = src + dst * (1 - src[..., 3:4])
    return dst


def glow(layer, sigma, gain=1.0):
    """Soft halo of a layer, added under it (return a new layer to put underneath)."""
    g = np.dstack([gaussian_filter(layer[..., i], sigma) for i in range(4)]) * gain
    return np.clip(g, 0, 1)


def with_glow(layer, sigma, gain=1.0, wide=0.0):
    base = glow(layer, sigma, gain)
    if wide:
        base = np.clip(base + glow(layer, sigma * 2.6, wide), 0, 1)
    out = base.copy()
    return over(out, layer)


def to_image(layer):
    a = layer[..., 3:4]
    straight = np.where(a > 1e-4, layer[..., :3] / np.maximum(a, 1e-4), 0)
    return Image.fromarray((np.dstack([np.clip(straight, 0, 1), np.clip(a, 0, 1)]) * 255 + 0.5).astype(np.uint8), "RGBA")


def hash01(i, salt=0):
    return (math.sin((i + 1) * 12.9898 + salt * 78.233) * 43758.5453) % 1.0


def rotate_point(x, y, cx, cy, ang):
    c, s = math.cos(ang), math.sin(ang)
    return cx + (x - cx) * c - (y - cy) * s, cy + (x - cx) * s + (y - cy) * c


def polygon_layer(size, polys, ss=4):
    """Polygons drawn at 4x and shrunk: used where a signed distance would be fiddly (petals, leaves, crystals)."""
    w, h = size
    img = Image.new("L", (w * ss, h * ss), 0)
    d = ImageDraw.Draw(img)
    for poly in polys:
        d.polygon([(x * ss, y * ss) for x, y in poly], fill=255)
    m = np.asarray(img.resize((w, h), Image.LANCZOS), float) / 255
    return m


def star4(x, y, cx, cy, r, squash=0.18):
    """Coverage of a four-point sparkle centred on (cx, cy), reaching r along the axes."""
    dx, dy = np.abs(x - cx) / r, np.abs(y - cy) / r
    d = (dx ** 0.5 + dy ** 0.5) ** 2 - 1  # astroid-like
    return smooth(0.35, -0.0, d) * (1 - smooth(0.9, 1.0, np.maximum(dx, dy)))


