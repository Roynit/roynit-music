"""Draws a contact sheet of every downloaded icon, to choose from. Writes assets/shop/_icons.png (git-ignored)."""
import glob
import os
import sys

sys.path.insert(0, os.path.dirname(__file__))
from PIL import Image, ImageDraw

import shop_icons

names = sorted(os.path.relpath(f, shop_icons.SRC).replace(os.sep, "/")[:-4] for f in glob.glob(str(shop_icons.SRC) + "/*/*.svg"))
cols, cell = 7, 200
rows = (len(names) + cols - 1) // cols
sheet = Image.new("RGB", (cols * cell, rows * (cell + 16)), (30, 30, 38))
d = ImageDraw.Draw(sheet)
for i, n in enumerate(names):
    m = shop_icons.icon_mask(n, cell - 10)
    im = Image.fromarray((m * 255).astype("uint8"))
    x, y = (i % cols) * cell + 5, (i // cols) * (cell + 16) + 5
    sheet.paste(Image.new("RGB", im.size, (240, 240, 250)), (x, y), im)
    d.text((x, y + cell - 12), n, fill=(180, 180, 200))
sheet.save(os.path.join(os.path.dirname(__file__), "..", "assets", "shop", "_icons.png"))
print(len(names))
