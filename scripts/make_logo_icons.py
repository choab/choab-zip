#!/usr/bin/env python3
"""
Derive every application-identity PNG from the hand-made source logo
`assets/choab-zip-logo.png`.

Outputs
-------
assets/icon.png            256 px  full logo  (window/taskbar icon, desktop entry)
assets/icon512.png         512 px  full logo
assets/icon64.png           64 px  full logo (small raster, e.g. previews)
assets/icons/app_mark.png   96 px  crimson "7" badge (in-app titlebar, 22 px slot)
assets/icons/archive16.png  96 px  same badge (archive rows, 17 px slot)
assets/icons/archive24.png 128 px  same badge (hi-dpi / legacy 24 px slot)

The build never reads these at runtime: they are embedded with
`include_bytes!`. Run this after changing the source logo:

    python3 scripts/make_logo_icons.py

The vectors/icons in `generate_all_icons.py` are unrelated to this script;
that one draws the toolbar/row/window-control icon set.
"""

import os

from PIL import Image, ImageDraw, ImageFilter

SRC = "assets/choab-zip-logo.png"
ICONS_DIR = "assets/icons"

# The logo's crimson, sampled from the source artwork.
FALLBACK_CRIMSON = (98, 24, 24, 255)


def load_trimmed():
    """Load the source logo and crop away its transparent border."""
    src = Image.open(SRC).convert("RGBA")
    bbox = src.split()[-1].getbbox()
    if not bbox:
        raise SystemExit(f"{SRC} has no visible pixels")
    return src.crop(bbox)


def full_logo_icon(trimmed, size):
    """The whole logo, centred on a transparent square canvas."""
    canvas = Image.new("RGBA", (size, size), (0, 0, 0, 0))
    fit = size * 0.86
    scale = fit / max(trimmed.size)
    w, h = (max(1, round(trimmed.width * scale)), max(1, round(trimmed.height * scale)))
    art = trimmed.resize((w, h), Image.Resampling.LANCZOS)
    canvas.alpha_composite(art, ((size - w) // 2, (size - h) // 2))
    return canvas


def extract_mark(trimmed):
    """Cut the leading '7' out of the logo and return (mask, crimson colour)."""
    # The 7 lives in the left part of the artwork; the R/ZIP lettering follows.
    crop = trimmed.crop((0, 0, int(trimmed.width * 0.47), trimmed.height))

    px = crop.load()
    w, h = crop.size
    mask = Image.new("L", (w, h), 0)
    mpx = mask.load()

    r_sum = g_sum = b_sum = count = 0
    for y in range(h):
        for x in range(w):
            r, g, b, a = px[x, y]
            # Crimson-ish and fully opaque; this rejects the black plates.
            if a > 128 and r > 45 and r > g + 22 and r > b + 22 and g < 90 and b < 90:
                mpx[x, y] = 255
                r_sum += r
                g_sum += g
                b_sum += b
                count += 1

    if count == 0:
        return None, FALLBACK_CRIMSON

    crimson = (r_sum // count, g_sum // count, b_sum // count, 255)
    return mask.crop(mask.getbbox()), crimson


def badge(mark, crimson, size):
    """Rounded black plate + white-outlined crimson 7, matching the logo style."""
    ss = 4  # supersample, then downsample for clean edges
    S = size * ss
    plate = Image.new("RGBA", (S, S), (0, 0, 0, 0))
    draw = ImageDraw.Draw(plate)
    draw.rounded_rectangle(
        [0, 0, S - 1, S - 1],
        radius=int(S * 0.24),
        fill=(10, 10, 10, 255),
        outline=(255, 255, 255, 255),
        width=max(1, int(S * 0.012)),
    )

    inner = int(S * 0.74)
    mh = inner
    mw = round(mark.width * mh / mark.height)
    if mw > inner:
        mw = inner
        mh = round(mark.height * mw / mark.width)

    m = mark.resize((mw, mh), Image.Resampling.LANCZOS)

    # Pad before dilating, otherwise the outline gets clipped on the edges
    # where the glyph touches its own bounding box.
    pad = max(3, int(S * 0.035))
    pw, ph = mw + pad * 2, mh + pad * 2
    padded = Image.new("L", (pw, ph), 0)
    padded.paste(m, (pad, pad))
    outline = padded.filter(ImageFilter.MaxFilter(max(3, (pad // 2) * 2 + 1)))

    white = Image.new("RGBA", (pw, ph), (255, 255, 255, 255))
    white.putalpha(outline)
    red = Image.new("RGBA", (pw, ph), crimson)
    red.putalpha(padded)

    ox, oy = (S - pw) // 2, (S - ph) // 2
    plate.alpha_composite(white, (ox, oy))
    plate.alpha_composite(red, (ox, oy))

    return plate.resize((size, size), Image.Resampling.LANCZOS)


def save(img, path, size=None):
    os.makedirs(os.path.dirname(os.path.abspath(path)), exist_ok=True)
    if size is not None and img.size != (size, size):
        img = img.resize((size, size), Image.Resampling.LANCZOS)
    img.save(path, "PNG", optimize=True)
    print(f"  -> {path} ({img.width}x{img.height})")


def main():
    trimmed = load_trimmed()
    mark, crimson = extract_mark(trimmed)
    if mark is None:
        raise SystemExit("could not isolate the '7' mark from the logo")

    print("Generating logo-derived icons...")

    logo512 = full_logo_icon(trimmed, 512)
    save(logo512, "assets/icon512.png")
    save(logo512, "assets/icon.png", 256)
    save(logo512, "assets/icon64.png", 64)

    save(badge(mark, crimson, 96), f"{ICONS_DIR}/app_mark.png")
    save(badge(mark, crimson, 96), f"{ICONS_DIR}/archive16.png")
    save(badge(mark, crimson, 128), f"{ICONS_DIR}/archive24.png")

    print("Done.")


if __name__ == "__main__":
    main()
