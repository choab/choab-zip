#!/usr/bin/env python3
"""Generate the choab-zip app icon: a black tile with a white zipper-7.

Greyscale by design, matching the in-app mark drawn by `design::paint_mark`.
Renders once at high resolution and downsamples, so every size stays crisp.

    ./scripts/make_app_icon.py
"""

import math
import pathlib

from PIL import Image, ImageDraw

ROOT = pathlib.Path(__file__).resolve().parent.parent
ASSETS = ROOT / "assets"

SIZE = 1024
INK = (10, 10, 10, 255)
PAPER = (255, 255, 255, 255)
SEAM = (140, 140, 140, 255)


def rounded(draw, box, radius, fill=None, outline=None, width=1):
    draw.rounded_rectangle(box, radius=radius, fill=fill, outline=outline, width=width)


def thick_line(draw, a, b, width, fill):
    """Line with round caps (PIL only rounds joins, not ends)."""
    draw.line([a, b], fill=fill, width=width)
    r = width / 2
    for x, y in (a, b):
        draw.ellipse([x - r, y - r, x + r, y + r], fill=fill)


def build(size, simple=False):
    img = Image.new("RGBA", (size, size), (0, 0, 0, 0))
    d = ImageDraw.Draw(img)

    margin = size * 0.06
    left, top, right, bottom = margin, margin, size - margin, size - margin
    radius = size * 0.20
    gap = (bottom - top) * 0.06
    mid = (top + bottom) / 2

    # Grey seam plate: only the gap between the two ink plates shows it.
    d.rounded_rectangle([left, top, right, bottom], radius=radius, fill=SEAM)

    d.rounded_rectangle(
        [left, top, right, mid - gap / 2],
        radius=radius,
        fill=INK,
        corners=(True, True, False, False),
    )
    d.rounded_rectangle(
        [left, mid + gap / 2, right, bottom],
        radius=radius,
        fill=INK,
        corners=(False, False, True, True),
    )

    # The 7 stays whole and bridges the seam.
    bar_y0, bar_y1 = top + (bottom - top) * 0.24, top + (bottom - top) * 0.31
    rounded(
        d,
        [left + (right - left) * 0.25, bar_y0, right - (right - left) * 0.24, bar_y1],
        radius=(bar_y1 - bar_y0) / 2,
        fill=PAPER,
    )
    start = (left + (right - left) * 0.70, bar_y1)
    end = (left + (right - left) * 0.40, top + (bottom - top) * 0.80)
    thick_line(d, start, end, int(size * 0.075), PAPER)
    d.rectangle(
        [right - (right - left) * 0.36, bar_y0, right - (right - left) * 0.27, bar_y1],
        fill=PAPER,
    )

    # Zipper teeth: notches bitten out of the left edge of the diagonal.
    # Below 48 px they turn into noise, so small sizes keep a clean 7.
    if not simple:
        dx, dy = end[0] - start[0], end[1] - start[1]
        length = math.hypot(dx, dy)
        nx, ny = -dy / length, dx / length
        for t in (0.40, 0.58, 0.76):
            cx = start[0] + dx * t - nx * size * 0.020
            cy = start[1] + dy * t - ny * size * 0.020
            a = (cx + nx * size * 0.045, cy + ny * size * 0.045)
            b = (cx - nx * size * 0.045, cy - ny * size * 0.045)
            thick_line(d, a, b, int(size * 0.014), INK)

    return img
def main():
    master = build(SIZE)
    sizes = (16, 22, 24, 32, 48, 64, 128, 256, 512)
    for px in sizes:
        out = master.resize((px, px), Image.LANCZOS)
        if px < 48:
            out = build(SIZE, simple=True).resize((px, px), Image.LANCZOS)
        name = "icon.png" if px == 256 else f"icon{px}.png"
        out.save(ASSETS / name)
        print(f"wrote assets/{name} ({px}×{px})")


if __name__ == "__main__":
    main()
