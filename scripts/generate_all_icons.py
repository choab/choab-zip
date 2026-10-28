#!/usr/bin/env python3
"""
Generate 2026-style professional colored icons for 7rust-zip.
Uses Pycairo for vector precision, multi-stop gradients, and micro-details,
and PIL for high-quality downsampling.
"""

import math
import os
import cairo
from PIL import Image

def hex_to_rgb(hex_str):
    hex_str = hex_str.lstrip('#')
    return tuple(int(hex_str[i:i+2], 16) / 255.0 for i in (0, 2, 4))

def hex_to_rgba(hex_str, alpha=1.0):
    r, g, b = hex_to_rgb(hex_str)
    return (r, g, b, alpha)

def round_rect(ctx, x, y, w, h, r):
    r = min(r, w/2.0, h/2.0)
    ctx.new_sub_path()
    ctx.arc(x + w - r, y + r, r, -math.pi/2, 0)
    ctx.arc(x + w - r, y + h - r, r, 0, math.pi/2)
    ctx.arc(x + r, y + h - r, r, math.pi/2, math.pi)
    ctx.arc(x + r, y + r, r, math.pi, 3*math.pi/2)
    ctx.close_path()

def create_surface(size=256):
    surf = cairo.ImageSurface(cairo.FORMAT_ARGB32, size, size)
    ctx = cairo.Context(surf)
    return surf, ctx

def save_icon(surf, dest_path, target_size=64):
    temp_path = '/tmp/cairo_export_temp.png'
    surf.write_to_png(temp_path)
    im = Image.open(temp_path)
    if im.size != (target_size, target_size):
        im = im.resize((target_size, target_size), Image.Resampling.LANCZOS)
    os.makedirs(os.path.dirname(os.path.abspath(dest_path)), exist_ok=True)
    im.save(dest_path, 'PNG', optimize=True)

# -------------------------------------------------------------
# Base squircle container with drop shadow & top specular highlight
# -------------------------------------------------------------
def draw_squircle_base(ctx, cx, cy, w, h, r, top_col, mid_col, bot_col, shadow_alpha=0.32):
    x, y = cx - w/2.0, cy - h/2.0

    # Drop shadow
    shadow_pat = cairo.RadialGradient(cx, cy + h*0.28, w*0.3, cx, cy + h*0.28, w*0.7)
    shadow_pat.add_color_stop_rgba(0, 0, 0, 0, shadow_alpha)
    shadow_pat.add_color_stop_rgba(1, 0, 0, 0, 0.0)
    ctx.set_source(shadow_pat)
    ctx.arc(cx, cy + h*0.28, w*0.7, 0, 2*math.pi)
    ctx.fill()

    # Main gradient fill
    pat = cairo.LinearGradient(cx, y, cx, y + h)
    pat.add_color_stop_rgb(0.0, *hex_to_rgb(top_col))
    pat.add_color_stop_rgb(0.55, *hex_to_rgb(mid_col))
    pat.add_color_stop_rgb(1.0, *hex_to_rgb(bot_col))
    ctx.set_source(pat)
    round_rect(ctx, x, y, w, h, r)
    ctx.fill()

    # Top specular rim highlight
    ctx.save()
    round_rect(ctx, x, y, w, h, r)
    ctx.clip()
    spec = cairo.LinearGradient(cx, y, cx, y + h * 0.45)
    spec.add_color_stop_rgba(0.0, 1, 1, 1, 0.60)
    spec.add_color_stop_rgba(0.4, 1, 1, 1, 0.15)
    spec.add_color_stop_rgba(1.0, 1, 1, 1, 0.0)
    ctx.set_source(spec)
    ctx.set_line_width(w * 0.04)
    round_rect(ctx, x + 1, y + 1, w - 2, h - 2, r - 1)
    ctx.stroke()
    ctx.restore()

    # Subtle border outline
    ctx.set_source_rgba(0, 0, 0, 0.18)
    ctx.set_line_width(w * 0.015)
    round_rect(ctx, x, y, w, h, r)
    ctx.stroke()

# =============================================================
# 1. ADD ICON
# =============================================================
def render_add(size=256):
    surf, ctx = create_surface(size)
    cx, cy = size/2.0, size/2.0
    w, h, r = size*0.82, size*0.82, size*0.25

    draw_squircle_base(ctx, cx, cy, w, h, r, '#34D399', '#059669', '#047857')

    # Plus sign
    pw = w * 0.52
    pt = w * 0.18
    pr = pt / 2.0

    # Plus drop shadow
    ctx.set_source_rgba(0, 0, 0, 0.28)
    s_off = size * 0.018
    round_rect(ctx, cx - pw/2, cy - pt/2 + s_off, pw, pt, pr)
    ctx.fill()
    round_rect(ctx, cx - pt/2, cy - pw/2 + s_off, pt, pw, pr)
    ctx.fill()

    # Plus glyph gradient (crisp white down to soft mint)
    plus_pat = cairo.LinearGradient(cx, cy - pw/2, cx, cy + pw/2)
    plus_pat.add_color_stop_rgb(0.0, 1, 1, 1)
    plus_pat.add_color_stop_rgb(1.0, 0.90, 0.99, 0.94)
    ctx.set_source(plus_pat)
    round_rect(ctx, cx - pw/2, cy - pt/2, pw, pt, pr)
    ctx.fill()
    round_rect(ctx, cx - pt/2, cy - pw/2, pt, pw, pr)
    ctx.fill()

    # Plus inner highlight
    ctx.save()
    round_rect(ctx, cx - pw/2, cy - pt/2, pw, pt, pr)
    round_rect(ctx, cx - pt/2, cy - pw/2, pt, pw, pr)
    ctx.clip()
    hi = cairo.LinearGradient(cx, cy - pw/2, cx, cy)
    hi.add_color_stop_rgba(0, 1, 1, 1, 0.75)
    hi.add_color_stop_rgba(1, 1, 1, 1, 0.0)
    ctx.set_source(hi)
    ctx.set_line_width(pt * 0.25)
    ctx.move_to(cx - pw/2, cy - pt/2 + pt*0.2)
    ctx.line_to(cx + pw/2, cy - pt/2 + pt*0.2)
    ctx.stroke()
    ctx.restore()

    return surf

# =============================================================
# 2. EXTRACT ICON
# =============================================================
def render_extract(size=256):
    surf, ctx = create_surface(size)
    cx, cy = size/2.0, size/2.0

    # Base: Open modern archive crate / container
    bw = size * 0.80
    bh = size * 0.50
    bx = cx - bw/2.0
    by = size * 0.42
    br = size * 0.12

    # Drop shadow
    shadow_pat = cairo.RadialGradient(cx, size*0.88, bw*0.2, cx, size*0.88, bw*0.65)
    shadow_pat.add_color_stop_rgba(0, 0, 0, 0, 0.35)
    shadow_pat.add_color_stop_rgba(1, 0, 0, 0, 0.0)
    ctx.set_source(shadow_pat)
    ctx.arc(cx, size*0.88, bw*0.65, 0, 2*math.pi)
    ctx.fill()

    # Box rear interior (dark slate depth)
    ctx.set_source_rgb(*hex_to_rgb('#0F172A'))
    round_rect(ctx, bx + bw*0.06, by - bh*0.14, bw*0.88, bh*0.4, br*0.6)
    ctx.fill()

    # Extraction arrow bursting out!
    aw = size * 0.20
    ctx.save()
    # Arrow drop shadow
    ctx.set_source_rgba(0, 0, 0, 0.32)
    s_off = size * 0.02
    ctx.move_to(cx, size * 0.08 + s_off)
    ctx.line_to(cx + size * 0.25, size * 0.32 + s_off)
    ctx.line_to(cx + aw*0.65, size * 0.32 + s_off)
    ctx.line_to(cx + aw*0.65, size * 0.58 + s_off)
    ctx.line_to(cx - aw*0.65, size * 0.58 + s_off)
    ctx.line_to(cx - aw*0.65, size * 0.32 + s_off)
    ctx.line_to(cx - size * 0.25, size * 0.32 + s_off)
    ctx.close_path()
    ctx.fill()

    # Arrow gradient (cyan to luminous blue)
    arr_pat = cairo.LinearGradient(cx, size * 0.08, cx, size * 0.58)
    arr_pat.add_color_stop_rgb(0.0, *hex_to_rgb('#E0F2FE'))
    arr_pat.add_color_stop_rgb(0.25, *hex_to_rgb('#38BDF8'))
    arr_pat.add_color_stop_rgb(0.65, *hex_to_rgb('#0284C7'))
    arr_pat.add_color_stop_rgb(1.0, *hex_to_rgb('#1D4ED8'))
    ctx.set_source(arr_pat)
    ctx.move_to(cx, size * 0.08)
    ctx.line_to(cx + size * 0.25, size * 0.32)
    ctx.line_to(cx + aw*0.65, size * 0.32)
    ctx.line_to(cx + aw*0.65, size * 0.58)
    ctx.line_to(cx - aw*0.65, size * 0.58)
    ctx.line_to(cx - aw*0.65, size * 0.32)
    ctx.line_to(cx - size * 0.25, size * 0.32)
    ctx.close_path()
    ctx.fill()

    # Arrow highlight
    ctx.set_source_rgba(1, 1, 1, 0.8)
    ctx.set_line_width(size * 0.02)
    ctx.move_to(cx - size * 0.22, size * 0.32)
    ctx.line_to(cx, size * 0.10)
    ctx.line_to(cx + size * 0.22, size * 0.32)
    ctx.stroke()
    ctx.restore()

    # Box front body: Warm golden amber / titanium container
    box_pat = cairo.LinearGradient(cx, by, cx, by + bh)
    box_pat.add_color_stop_rgb(0.0, *hex_to_rgb('#FDE68A'))
    box_pat.add_color_stop_rgb(0.3, *hex_to_rgb('#F59E0B'))
    box_pat.add_color_stop_rgb(1.0, *hex_to_rgb('#B45309'))
    ctx.set_source(box_pat)
    round_rect(ctx, bx, by, bw, bh, br)
    ctx.fill()

    # Top highlight on front rim
    ctx.save()
    round_rect(ctx, bx, by, bw, bh, br)
    ctx.clip()
    hi = cairo.LinearGradient(cx, by, cx, by + bh*0.25)
    hi.add_color_stop_rgba(0, 1, 1, 1, 0.70)
    hi.add_color_stop_rgba(1, 1, 1, 1, 0.0)
    ctx.set_source(hi)
    ctx.set_line_width(bw * 0.05)
    round_rect(ctx, bx+1, by+1, bw-2, bh-2, br-1)
    ctx.stroke()
    ctx.restore()

    # Archive zipper detail on the box front
    zw = bw * 0.15
    zh = bh * 0.65
    zx = cx - zw/2.0
    zy = by + bh*0.20
    ctx.set_source_rgb(*hex_to_rgb('#78350F'))
    round_rect(ctx, zx, zy, zw, zh, zw*0.3)
    ctx.fill()

    # Zipper teeth (metallic gold dashes)
    ctx.set_source_rgb(*hex_to_rgb('#FEF08A'))
    ctx.set_line_width(size * 0.022)
    for i in range(5):
        ty = zy + zh * 0.16 + i * (zh * 0.17)
        ctx.move_to(zx + zw*0.15, ty)
        ctx.line_to(zx + zw*0.85, ty)
        ctx.stroke()

    # Front box border
    ctx.set_source_rgba(0, 0, 0, 0.20)
    ctx.set_line_width(bw * 0.015)
    round_rect(ctx, bx, by, bw, bh, br)
    ctx.stroke()

    return surf

# =============================================================
# 3. TEST / VERIFY ICON
# =============================================================
def render_test(size=256):
    surf, ctx = create_surface(size)
    cx, cy = size/2.0, size/2.0
    sw = size * 0.76
    sh = size * 0.84

    # Drop shadow
    shadow_pat = cairo.RadialGradient(cx, size*0.90, sw*0.2, cx, size*0.90, sw*0.65)
    shadow_pat.add_color_stop_rgba(0, 0, 0, 0, 0.35)
    shadow_pat.add_color_stop_rgba(1, 0, 0, 0, 0.0)
    ctx.set_source(shadow_pat)
    ctx.arc(cx, size*0.90, sw*0.65, 0, 2*math.pi)
    ctx.fill()

    def shield_path():
        ctx.new_sub_path()
        top_y = cy - sh * 0.46
        mid_y = cy + sh * 0.08
        bot_y = cy + sh * 0.48
        r = sw * 0.12
        ctx.move_to(cx - sw/2 + r, top_y)
        ctx.line_to(cx + sw/2 - r, top_y)
        ctx.arc(cx + sw/2 - r, top_y + r, r, -math.pi/2, 0)
        ctx.line_to(cx + sw/2, mid_y)
        ctx.curve_to(cx + sw/2, bot_y - sh*0.12, cx + sw*0.18, bot_y - sh*0.04, cx, bot_y)
        ctx.curve_to(cx - sw*0.18, bot_y - sh*0.04, cx - sw/2, bot_y - sh*0.12, cx - sw/2, mid_y)
        ctx.line_to(cx - sw/2, top_y + r)
        ctx.arc(cx - sw/2 + r, top_y + r, r, math.pi, 3*math.pi/2)
        ctx.close_path()

    # Shield fill: Sapphire cobalt to deep navy
    shield_pat = cairo.LinearGradient(cx, cy - sh/2, cx, cy + sh/2)
    shield_pat.add_color_stop_rgb(0.0, *hex_to_rgb('#60A5FA'))
    shield_pat.add_color_stop_rgb(0.4, *hex_to_rgb('#2563EB'))
    shield_pat.add_color_stop_rgb(1.0, *hex_to_rgb('#1E1B4B'))
    ctx.set_source(shield_pat)
    shield_path()
    ctx.fill()

    # Glass reflection sheen
    ctx.save()
    shield_path()
    ctx.clip()
    sheen = cairo.LinearGradient(cx - sw/2, cy - sh/2, cx + sw/2, cy)
    sheen.add_color_stop_rgba(0.0, 1, 1, 1, 0.45)
    sheen.add_color_stop_rgba(0.4, 1, 1, 1, 0.15)
    sheen.add_color_stop_rgba(1.0, 1, 1, 1, 0.0)
    ctx.set_source(sheen)
    ctx.move_to(cx - sw/2, cy - sh/2)
    ctx.line_to(cx + sw/2, cy - sh/2)
    ctx.line_to(cx - sw/2, cy + sh*0.1)
    ctx.close_path()
    ctx.fill()

    # Top highlight rim
    hi = cairo.LinearGradient(cx, cy - sh*0.46, cx, cy - sh*0.2)
    hi.add_color_stop_rgba(0, 1, 1, 1, 0.65)
    hi.add_color_stop_rgba(1, 1, 1, 1, 0.0)
    ctx.set_source(hi)
    ctx.set_line_width(sw * 0.04)
    shield_path()
    ctx.stroke()
    ctx.restore()

    # Shield border stroke
    ctx.set_source_rgba(0, 0, 0, 0.25)
    ctx.set_line_width(sw * 0.02)
    shield_path()
    ctx.stroke()

    # Luminous Mint/Emerald Checkmark
    ctx.save()
    ctx.set_line_cap(cairo.LINE_CAP_ROUND)
    ctx.set_line_join(cairo.LINE_JOIN_ROUND)
    cw = sw * 0.16
    ctx.set_line_width(cw)
    s_off = size * 0.02

    # Drop shadow
    ctx.set_source_rgba(0, 0, 0, 0.35)
    ctx.move_to(cx - sw*0.26, cy + s_off)
    ctx.line_to(cx - sw*0.06, cy + sh*0.16 + s_off)
    ctx.line_to(cx + sw*0.28, cy - sh*0.16 + s_off)
    ctx.stroke()

    # Checkmark gradient (pure mint-white to emerald)
    chk_pat = cairo.LinearGradient(cx - sw*0.26, cy - sh*0.16, cx + sw*0.28, cy + sh*0.16)
    chk_pat.add_color_stop_rgb(0.0, *hex_to_rgb('#D1FAE5'))
    chk_pat.add_color_stop_rgb(0.4, *hex_to_rgb('#34D399'))
    chk_pat.add_color_stop_rgb(1.0, *hex_to_rgb('#059669'))
    ctx.set_source(chk_pat)
    ctx.move_to(cx - sw*0.26, cy)
    ctx.line_to(cx - sw*0.06, cy + sh*0.16)
    ctx.line_to(cx + sw*0.28, cy - sh*0.16)
    ctx.stroke()

    # Checkmark white inner core
    ctx.set_source_rgba(1, 1, 1, 0.90)
    ctx.set_line_width(cw * 0.35)
    ctx.move_to(cx - sw*0.26, cy)
    ctx.line_to(cx - sw*0.06, cy + sh*0.16)
    ctx.line_to(cx + sw*0.28, cy - sh*0.16)
    ctx.stroke()
    ctx.restore()

    return surf

# =============================================================
# 4. COPY ICON
# =============================================================
def render_copy(size=256):
    surf, ctx = create_surface(size)
    cx, cy = size/2.0, size/2.0
    dw = size * 0.50
    dh = size * 0.64
    dr = size * 0.10

    # Back card (indigo/violet offset to top right)
    bx = cx - dw*0.18
    by = cy - dh*0.60
    shadow_pat = cairo.RadialGradient(bx + dw*0.5, by + dh*0.6, dw*0.3, bx + dw*0.5, by + dh*0.6, dw*0.8)
    shadow_pat.add_color_stop_rgba(0, 0, 0, 0, 0.35)
    shadow_pat.add_color_stop_rgba(1, 0, 0, 0, 0.0)
    ctx.set_source(shadow_pat)
    ctx.arc(bx + dw*0.5, by + dh*0.6, dw*0.8, 0, 2*math.pi)
    ctx.fill()

    bpat = cairo.LinearGradient(bx, by, bx + dw, by + dh)
    bpat.add_color_stop_rgb(0.0, *hex_to_rgb('#818CF8'))
    bpat.add_color_stop_rgb(1.0, *hex_to_rgb('#4338CA'))
    ctx.set_source(bpat)
    round_rect(ctx, bx, by, dw, dh, dr)
    ctx.fill()
    ctx.set_source_rgba(0, 0, 0, 0.2)
    ctx.set_line_width(size * 0.012)
    round_rect(ctx, bx, by, dw, dh, dr)
    ctx.stroke()

    # Front card (vibrant violet / purple)
    fx = cx - dw*0.82
    fy = cy - dh*0.38

    f_shad = cairo.RadialGradient(fx + dw*0.5, fy + dh*0.5, dw*0.3, fx + dw*0.5, fy + dh*0.5, dw*0.75)
    f_shad.add_color_stop_rgba(0, 0, 0, 0, 0.40)
    f_shad.add_color_stop_rgba(1, 0, 0, 0, 0.0)
    ctx.set_source(f_shad)
    ctx.arc(fx + dw*0.5, fy + dh*0.5, dw*0.75, 0, 2*math.pi)
    ctx.fill()

    fpat = cairo.LinearGradient(fx, fy, fx + dw, fy + dh)
    fpat.add_color_stop_rgb(0.0, *hex_to_rgb('#C084FC'))
    fpat.add_color_stop_rgb(0.5, *hex_to_rgb('#9333EA'))
    fpat.add_color_stop_rgb(1.0, *hex_to_rgb('#6B21A8'))
    ctx.set_source(fpat)
    round_rect(ctx, fx, fy, dw, dh, dr)
    ctx.fill()

    # Specular rim
    ctx.save()
    round_rect(ctx, fx, fy, dw, dh, dr)
    ctx.clip()
    hi = cairo.LinearGradient(fx, fy, fx, fy + dh*0.35)
    hi.add_color_stop_rgba(0, 1, 1, 1, 0.60)
    hi.add_color_stop_rgba(1, 1, 1, 1, 0.0)
    ctx.set_source(hi)
    ctx.set_line_width(dw * 0.06)
    round_rect(ctx, fx+1, fy+1, dw-2, dh-2, dr-1)
    ctx.stroke()
    ctx.restore()

    # Front card content stripes
    ctx.set_source_rgba(1, 1, 1, 0.90)
    ctx.set_line_cap(cairo.LINE_CAP_ROUND)
    ctx.set_line_width(dh * 0.08)
    ctx.move_to(fx + dw*0.22, fy + dh*0.30)
    ctx.line_to(fx + dw*0.78, fy + dh*0.30)
    ctx.stroke()
    ctx.set_source_rgba(1, 1, 1, 0.70)
    ctx.move_to(fx + dw*0.22, fy + dh*0.50)
    ctx.line_to(fx + dw*0.70, fy + dh*0.50)
    ctx.stroke()
    ctx.move_to(fx + dw*0.22, fy + dh*0.70)
    ctx.line_to(fx + dw*0.55, fy + dh*0.70)
    ctx.stroke()

    ctx.set_source_rgba(0, 0, 0, 0.2)
    ctx.set_line_width(size * 0.012)
    round_rect(ctx, fx, fy, dw, dh, dr)
    ctx.stroke()

    return surf

# =============================================================
# 5. MOVE ICON
# =============================================================
def render_move(size=256):
    surf, ctx = create_surface(size)
    cx, cy = size/2.0, size/2.0

    cw = size * 0.44
    ch = size * 0.56
    cx_box = size * 0.12
    cy_box = size * 0.22
    cr = size * 0.09

    shad = cairo.RadialGradient(cx_box + cw/2, cy_box + ch*0.6, cw*0.3, cx_box + cw/2, cy_box + ch*0.6, cw*0.75)
    shad.add_color_stop_rgba(0, 0, 0, 0, 0.35)
    shad.add_color_stop_rgba(1, 0, 0, 0, 0.0)
    ctx.set_source(shad)
    ctx.arc(cx_box + cw/2, cy_box + ch*0.6, cw*0.75, 0, 2*math.pi)
    ctx.fill()

    cpat = cairo.LinearGradient(cx_box, cy_box, cx_box + cw, cy_box + ch)
    cpat.add_color_stop_rgb(0.0, *hex_to_rgb('#A855F7'))
    cpat.add_color_stop_rgb(0.6, *hex_to_rgb('#7E22CE'))
    cpat.add_color_stop_rgb(1.0, *hex_to_rgb('#581C87'))
    ctx.set_source(cpat)
    round_rect(ctx, cx_box, cy_box, cw, ch, cr)
    ctx.fill()

    ctx.save()
    round_rect(ctx, cx_box, cy_box, cw, ch, cr)
    ctx.clip()
    hi = cairo.LinearGradient(cx_box, cy_box, cx_box, cy_box + ch*0.4)
    hi.add_color_stop_rgba(0, 1, 1, 1, 0.55)
    hi.add_color_stop_rgba(1, 1, 1, 1, 0.0)
    ctx.set_source(hi)
    ctx.set_line_width(cw * 0.08)
    round_rect(ctx, cx_box+1, cy_box+1, cw-2, ch-2, cr-1)
    ctx.stroke()
    ctx.restore()

    ctx.set_source_rgba(1, 1, 1, 0.75)
    ctx.set_line_cap(cairo.LINE_CAP_ROUND)
    ctx.set_line_width(ch * 0.08)
    ctx.move_to(cx_box + cw*0.25, cy_box + ch*0.35)
    ctx.line_to(cx_box + cw*0.75, cy_box + ch*0.35)
    ctx.stroke()
    ctx.move_to(cx_box + cw*0.25, cy_box + ch*0.58)
    ctx.line_to(cx_box + cw*0.65, cy_box + ch*0.58)
    ctx.stroke()

    # Dynamic sweeping curved arrow towards bottom right
    ctx.save()
    s_off = size * 0.02
    ctx.set_source_rgba(0, 0, 0, 0.28)
    ctx.set_line_width(size * 0.13)
    ctx.set_line_cap(cairo.LINE_CAP_ROUND)
    ctx.move_to(cx_box + cw*0.6, cy_box + ch*0.75 + s_off)
    ctx.curve_to(cx + size*0.08, cy_box + ch*0.75 + s_off, size*0.60, size*0.72 + s_off, size*0.70, size*0.72 + s_off)
    ctx.stroke()

    arr_pat = cairo.LinearGradient(size*0.35, size*0.45, size*0.85, size*0.75)
    arr_pat.add_color_stop_rgb(0.0, *hex_to_rgb('#22D3EE'))
    arr_pat.add_color_stop_rgb(0.5, *hex_to_rgb('#06B6D4'))
    arr_pat.add_color_stop_rgb(1.0, *hex_to_rgb('#3B82F6'))
    ctx.set_source(arr_pat)
    ctx.set_line_width(size * 0.12)
    ctx.move_to(cx_box + cw*0.6, cy_box + ch*0.75)
    ctx.curve_to(cx + size*0.08, cy_box + ch*0.75, size*0.60, size*0.72, size*0.70, size*0.72)
    ctx.stroke()

    ah_pat = cairo.LinearGradient(size*0.62, size*0.50, size*0.92, size*0.85)
    ah_pat.add_color_stop_rgb(0.0, *hex_to_rgb('#67E8F9'))
    ah_pat.add_color_stop_rgb(1.0, *hex_to_rgb('#2563EB'))
    ctx.set_source(ah_pat)
    ctx.move_to(size*0.92, size*0.72)
    ctx.line_to(size*0.66, size*0.54)
    ctx.line_to(size*0.70, size*0.72)
    ctx.line_to(size*0.66, size*0.90)
    ctx.close_path()
    ctx.fill()

    ctx.set_source_rgba(1, 1, 1, 0.75)
    ctx.set_line_width(size * 0.02)
    ctx.move_to(size*0.68, size*0.56)
    ctx.line_to(size*0.90, size*0.72)
    ctx.stroke()
    ctx.restore()

    return surf

# =============================================================
# 6. DELETE / TRASH ICON
# =============================================================
def render_delete(size=256):
    surf, ctx = create_surface(size)
    cx, cy = size/2.0, size/2.0

    shadow_pat = cairo.RadialGradient(cx, size*0.90, size*0.15, cx, size*0.90, size*0.50)
    shadow_pat.add_color_stop_rgba(0, 0, 0, 0, 0.35)
    shadow_pat.add_color_stop_rgba(1, 0, 0, 0, 0.0)
    ctx.set_source(shadow_pat)
    ctx.arc(cx, size*0.90, size*0.50, 0, 2*math.pi)
    ctx.fill()

    bw_top = size * 0.58
    bw_bot = size * 0.46
    bh = size * 0.52
    by_top = size * 0.38
    by_bot = by_top + bh
    cr = size * 0.08

    def canister_path():
        ctx.new_sub_path()
        ctx.move_to(cx - bw_top/2, by_top)
        ctx.line_to(cx + bw_top/2, by_top)
        ctx.line_to(cx + bw_bot/2, by_bot - cr)
        ctx.arc(cx + bw_bot/2 - cr, by_bot - cr, cr, 0, math.pi/2)
        ctx.line_to(cx - bw_bot/2 + cr, by_bot)
        ctx.arc(cx - bw_bot/2 + cr, by_bot - cr, cr, math.pi/2, math.pi)
        ctx.close_path()

    c_pat = cairo.LinearGradient(cx, by_top, cx, by_bot)
    c_pat.add_color_stop_rgb(0.0, *hex_to_rgb('#FB7185'))
    c_pat.add_color_stop_rgb(0.4, *hex_to_rgb('#E11D48'))
    c_pat.add_color_stop_rgb(1.0, *hex_to_rgb('#9F1239'))
    ctx.set_source(c_pat)
    canister_path()
    ctx.fill()

    ctx.save()
    canister_path()
    ctx.clip()
    hi = cairo.LinearGradient(cx, by_top, cx, by_top + bh*0.25)
    hi.add_color_stop_rgba(0, 1, 1, 1, 0.55)
    hi.add_color_stop_rgba(1, 1, 1, 1, 0.0)
    ctx.set_source(hi)
    ctx.set_line_width(size * 0.04)
    canister_path()
    ctx.stroke()

    # Vertical ribbed grooves on canister body
    ctx.set_source_rgba(0, 0, 0, 0.22)
    ctx.set_line_cap(cairo.LINE_CAP_ROUND)
    ctx.set_line_width(size * 0.035)
    for off in [-size*0.13, 0.0, size*0.13]:
        ctx.move_to(cx + off*0.9, by_top + bh*0.18)
        ctx.line_to(cx + off*0.75, by_bot - bh*0.18)
        ctx.stroke()
    ctx.set_source_rgba(1, 1, 1, 0.25)
    ctx.set_line_width(size * 0.015)
    for off in [-size*0.13, 0.0, size*0.13]:
        ctx.move_to(cx + off*0.9 + size*0.01, by_top + bh*0.18)
        ctx.line_to(cx + off*0.75 + size*0.01, by_bot - bh*0.18)
        ctx.stroke()
    ctx.restore()

    # Floating tilted lid
    ctx.save()
    ctx.translate(cx, by_top - size*0.06)
    ctx.rotate(-0.10)

    hw = size * 0.22
    hh = size * 0.08
    ctx.set_source_rgb(*hex_to_rgb('#FDA4AF'))
    round_rect(ctx, -hw/2, -hh - size*0.04, hw, hh, size*0.03)
    ctx.fill()

    lw = size * 0.70
    lh = size * 0.09
    lr = size * 0.04
    lid_pat = cairo.LinearGradient(0, -lh/2, 0, lh/2)
    lid_pat.add_color_stop_rgb(0.0, *hex_to_rgb('#FECDD3'))
    lid_pat.add_color_stop_rgb(0.5, *hex_to_rgb('#F43F5E'))
    lid_pat.add_color_stop_rgb(1.0, *hex_to_rgb('#BE123C'))
    ctx.set_source(lid_pat)
    round_rect(ctx, -lw/2, -lh/2, lw, lh, lr)
    ctx.fill()

    ctx.set_source_rgba(1, 1, 1, 0.7)
    ctx.set_line_width(size * 0.015)
    ctx.move_to(-lw/2 + lr, -lh/2 + size*0.01)
    ctx.line_to(lw/2 - lr, -lh/2 + size*0.01)
    ctx.stroke()
    ctx.restore()

    return surf

# =============================================================
# 7. INFO ICON
# =============================================================
def render_info(size=256):
    surf, ctx = create_surface(size)
    cx, cy = size/2.0, size/2.0
    r = size * 0.40

    shadow_pat = cairo.RadialGradient(cx, cy + r*0.2, r*0.6, cx, cy + r*0.2, r*1.15)
    shadow_pat.add_color_stop_rgba(0, 0, 0, 0, 0.32)
    shadow_pat.add_color_stop_rgba(1, 0, 0, 0, 0.0)
    ctx.set_source(shadow_pat)
    ctx.arc(cx, cy + r*0.2, r*1.15, 0, 2*math.pi)
    ctx.fill()

    pat = cairo.LinearGradient(cx, cy - r, cx, cy + r)
    pat.add_color_stop_rgb(0.0, *hex_to_rgb('#38BDF8'))
    pat.add_color_stop_rgb(0.55, *hex_to_rgb('#0284C7'))
    pat.add_color_stop_rgb(1.0, *hex_to_rgb('#075985'))
    ctx.set_source(pat)
    ctx.arc(cx, cy, r, 0, 2*math.pi)
    ctx.fill()

    ctx.save()
    ctx.arc(cx, cy, r, 0, 2*math.pi)
    ctx.clip()
    hi = cairo.LinearGradient(cx, cy - r, cx, cy)
    hi.add_color_stop_rgba(0, 1, 1, 1, 0.60)
    hi.add_color_stop_rgba(0.6, 1, 1, 1, 0.15)
    hi.add_color_stop_rgba(1, 1, 1, 1, 0.0)
    ctx.set_source(hi)
    ctx.set_line_width(r * 0.12)
    ctx.arc(cx, cy, r - r*0.06, 0, 2*math.pi)
    ctx.stroke()
    ctx.restore()

    ctx.set_source_rgba(0, 0, 0, 0.18)
    ctx.set_line_width(size * 0.015)
    ctx.arc(cx, cy, r, 0, 2*math.pi)
    ctx.stroke()

    # Lowercase 'i' glyph
    ctx.set_source_rgba(0, 0, 0, 0.30)
    s_off = size * 0.02
    dot_r = r * 0.15
    dot_y = cy - r * 0.42
    ctx.arc(cx, dot_y + s_off, dot_r, 0, 2*math.pi)
    ctx.fill()
    stem_w = r * 0.28
    stem_h = r * 0.72
    stem_y = cy - r * 0.12
    round_rect(ctx, cx - stem_w/2, stem_y + s_off, stem_w, stem_h, stem_w/2)
    ctx.fill()

    gly_pat = cairo.LinearGradient(cx, dot_y - dot_r, cx, stem_y + stem_h)
    gly_pat.add_color_stop_rgb(0.0, 1, 1, 1)
    gly_pat.add_color_stop_rgb(1.0, *hex_to_rgb('#E0F2FE'))
    ctx.set_source(gly_pat)
    ctx.arc(cx, dot_y, dot_r, 0, 2*math.pi)
    ctx.fill()
    round_rect(ctx, cx - stem_w/2, stem_y, stem_w, stem_h, stem_w/2)
    ctx.fill()

    return surf

# =============================================================
# 8. UP ARROW ICON
# =============================================================
def render_up(size=256):
    surf, ctx = create_surface(size)
    cx, cy = size/2.0, size/2.0
    r = size * 0.40

    shadow_pat = cairo.RadialGradient(cx, cy + r*0.2, r*0.6, cx, cy + r*0.2, r*1.15)
    shadow_pat.add_color_stop_rgba(0, 0, 0, 0, 0.30)
    shadow_pat.add_color_stop_rgba(1, 0, 0, 0, 0.0)
    ctx.set_source(shadow_pat)
    ctx.arc(cx, cy + r*0.2, r*1.15, 0, 2*math.pi)
    ctx.fill()

    pat = cairo.LinearGradient(cx, cy - r, cx, cy + r)
    pat.add_color_stop_rgb(0.0, *hex_to_rgb('#60A5FA'))
    pat.add_color_stop_rgb(0.5, *hex_to_rgb('#2563EB'))
    pat.add_color_stop_rgb(1.0, *hex_to_rgb('#1E3A8A'))
    ctx.set_source(pat)
    ctx.arc(cx, cy, r, 0, 2*math.pi)
    ctx.fill()

    ctx.save()
    ctx.arc(cx, cy, r, 0, 2*math.pi)
    ctx.clip()
    hi = cairo.LinearGradient(cx, cy - r, cx, cy)
    hi.add_color_stop_rgba(0, 1, 1, 1, 0.55)
    hi.add_color_stop_rgba(1, 1, 1, 1, 0.0)
    ctx.set_source(hi)
    ctx.set_line_width(r * 0.12)
    ctx.arc(cx, cy, r - r*0.06, 0, 2*math.pi)
    ctx.stroke()
    ctx.restore()

    ctx.save()
    s_off = size * 0.02
    ctx.set_line_cap(cairo.LINE_CAP_ROUND)
    ctx.set_line_join(cairo.LINE_JOIN_ROUND)
    lw = r * 0.26
    ctx.set_line_width(lw)

    ctx.set_source_rgba(0, 0, 0, 0.30)
    ctx.move_to(cx - r*0.48, cy + r*0.12 + s_off)
    ctx.line_to(cx, cy - r*0.36 + s_off)
    ctx.line_to(cx + r*0.48, cy + r*0.12 + s_off)
    ctx.stroke()
    ctx.move_to(cx, cy - r*0.36 + s_off)
    ctx.line_to(cx, cy + r*0.48 + s_off)
    ctx.stroke()

    ctx.set_source_rgba(1, 1, 1, 0.98)
    ctx.move_to(cx - r*0.48, cy + r*0.12)
    ctx.line_to(cx, cy - r*0.36)
    ctx.line_to(cx + r*0.48, cy + r*0.12)
    ctx.stroke()
    ctx.move_to(cx, cy - r*0.36)
    ctx.line_to(cx, cy + r*0.48)
    ctx.stroke()
    ctx.restore()

    return surf

# =============================================================
# 9. REFRESH ICON
# =============================================================
def render_refresh(size=256):
    surf, ctx = create_surface(size)
    cx, cy = size/2.0, size/2.0
    r = size * 0.40

    shadow_pat = cairo.RadialGradient(cx, cy + r*0.2, r*0.6, cx, cy + r*0.2, r*1.15)
    shadow_pat.add_color_stop_rgba(0, 0, 0, 0, 0.30)
    shadow_pat.add_color_stop_rgba(1, 0, 0, 0, 0.0)
    ctx.set_source(shadow_pat)
    ctx.arc(cx, cy + r*0.2, r*1.15, 0, 2*math.pi)
    ctx.fill()

    pat = cairo.LinearGradient(cx, cy - r, cx, cy + r)
    pat.add_color_stop_rgb(0.0, *hex_to_rgb('#2DD4BF'))
    pat.add_color_stop_rgb(0.5, *hex_to_rgb('#0D9488'))
    pat.add_color_stop_rgb(1.0, *hex_to_rgb('#064E3B'))
    ctx.set_source(pat)
    ctx.arc(cx, cy, r, 0, 2*math.pi)
    ctx.fill()

    ctx.save()
    ctx.arc(cx, cy, r, 0, 2*math.pi)
    ctx.clip()
    hi = cairo.LinearGradient(cx, cy - r, cx, cy)
    hi.add_color_stop_rgba(0, 1, 1, 1, 0.55)
    hi.add_color_stop_rgba(1, 1, 1, 1, 0.0)
    ctx.set_source(hi)
    ctx.set_line_width(r * 0.12)
    ctx.arc(cx, cy, r - r*0.06, 0, 2*math.pi)
    ctx.stroke()
    ctx.restore()

    ctx.save()
    ar_r = r * 0.48
    lw = r * 0.18
    ctx.set_line_width(lw)
    ctx.set_line_cap(cairo.LINE_CAP_ROUND)

    ctx.set_source_rgba(1, 1, 1, 0.95)
    ctx.arc(cx, cy, ar_r, -math.pi * 0.85, -math.pi * 0.10)
    ctx.stroke()
    a1_x = cx + ar_r * math.cos(-math.pi * 0.10)
    a1_y = cy + ar_r * math.sin(-math.pi * 0.10)
    ctx.move_to(a1_x + r*0.14, a1_y)
    ctx.line_to(a1_x, a1_y + r*0.14)
    ctx.line_to(a1_x - r*0.14, a1_y)
    ctx.close_path()
    ctx.fill()

    ctx.arc(cx, cy, ar_r, math.pi * 0.15, math.pi * 0.90)
    ctx.stroke()
    a2_x = cx + ar_r * math.cos(math.pi * 0.90)
    a2_y = cy + ar_r * math.sin(math.pi * 0.90)
    ctx.move_to(a2_x - r*0.14, a2_y)
    ctx.line_to(a2_x, a2_y - r*0.14)
    ctx.line_to(a2_x + r*0.14, a2_y)
    ctx.close_path()
    ctx.fill()
    ctx.restore()

    return surf

# =============================================================
# 10. FOLDER ICON
# =============================================================
def render_folder(size=256, with_up_badge=False):
    surf, ctx = create_surface(size)
    cx, cy = size/2.0, size/2.0

    fw = size * 0.84
    fh = size * 0.64
    fx = cx - fw/2.0
    fy = cy - fh/2.0 + size * 0.04
    fr = size * 0.10

    shadow_pat = cairo.RadialGradient(cx, fy + fh*0.9, fw*0.2, cx, fy + fh*0.9, fw*0.6)
    shadow_pat.add_color_stop_rgba(0, 0, 0, 0, 0.32)
    shadow_pat.add_color_stop_rgba(1, 0, 0, 0, 0.0)
    ctx.set_source(shadow_pat)
    ctx.arc(cx, fy + fh*0.9, fw*0.6, 0, 2*math.pi)
    ctx.fill()

    tab_w = fw * 0.44
    tab_h = fh * 0.22
    tab_r = size * 0.07

    def back_folder():
        ctx.new_sub_path()
        ctx.move_to(fx + tab_r, fy - tab_h)
        ctx.line_to(fx + tab_w - tab_r, fy - tab_h)
        ctx.arc(fx + tab_w - tab_r, fy - tab_h + tab_r, tab_r, -math.pi/2, 0)
        ctx.curve_to(fx + tab_w + tab_r, fy - tab_h*0.3, fx + tab_w + tab_r*1.5, fy, fx + tab_w + tab_r*3, fy)
        ctx.line_to(fx + fw - fr, fy)
        ctx.arc(fx + fw - fr, fy + fr, fr, -math.pi/2, 0)
        ctx.line_to(fx + fw, fy + fh - fr)
        ctx.arc(fx + fw - fr, fy + fh - fr, fr, 0, math.pi/2)
        ctx.line_to(fx + fr, fy + fh)
        ctx.arc(fx + fr, fy + fh - fr, fr, math.pi/2, math.pi)
        ctx.line_to(fx, fy - tab_h + tab_r)
        ctx.arc(fx + tab_r, fy - tab_h + tab_r, tab_r, math.pi, 3*math.pi/2)
        ctx.close_path()

    back_pat = cairo.LinearGradient(cx, fy - tab_h, cx, fy + fh)
    back_pat.add_color_stop_rgb(0.0, *hex_to_rgb('#D97706'))
    back_pat.add_color_stop_rgb(1.0, *hex_to_rgb('#92400E'))
    ctx.set_source(back_pat)
    back_folder()
    ctx.fill()

    # Peeking document
    doc_w = fw * 0.68
    doc_h = fh * 0.40
    doc_x = cx - doc_w/2.0
    doc_y = fy - tab_h * 0.4
    doc_r = size * 0.04
    ctx.set_source_rgb(*hex_to_rgb('#F8FAFC'))
    round_rect(ctx, doc_x, doc_y, doc_w, doc_h, doc_r)
    ctx.fill()
    ctx.set_source_rgb(*hex_to_rgb('#38BDF8'))
    round_rect(ctx, doc_x + doc_w*0.15, doc_y + doc_h*0.25, doc_w*0.7, doc_h*0.18, doc_h*0.09)
    ctx.fill()

    # Front flap
    front_h = fh * 0.74
    front_y = fy + fh - front_h

    def front_flap():
        ctx.new_sub_path()
        ctx.move_to(fx, front_y + fr)
        ctx.arc(fx + fr, front_y + fr, fr, math.pi, 3*math.pi/2)
        ctx.line_to(fx + fw - fr, front_y)
        ctx.arc(fx + fw - fr, front_y + fr, fr, -math.pi/2, 0)
        ctx.line_to(fx + fw, fy + fh - fr)
        ctx.arc(fx + fw - fr, fy + fh - fr, fr, 0, math.pi/2)
        ctx.line_to(fx + fr, fy + fh)
        ctx.arc(fx + fr, fy + fh - fr, fr, math.pi/2, math.pi)
        ctx.close_path()

    front_pat = cairo.LinearGradient(cx, front_y, cx, fy + fh)
    front_pat.add_color_stop_rgb(0.0, *hex_to_rgb('#FDE68A'))
    front_pat.add_color_stop_rgb(0.2, *hex_to_rgb('#FBBF24'))
    front_pat.add_color_stop_rgb(0.7, *hex_to_rgb('#F59E0B'))
    front_pat.add_color_stop_rgb(1.0, *hex_to_rgb('#D97706'))
    ctx.set_source(front_pat)
    front_flap()
    ctx.fill()

    ctx.save()
    front_flap()
    ctx.clip()
    hi = cairo.LinearGradient(cx, front_y, cx, front_y + front_h*0.3)
    hi.add_color_stop_rgba(0, 1, 1, 1, 0.75)
    hi.add_color_stop_rgba(1, 1, 1, 1, 0.0)
    ctx.set_source(hi)
    ctx.set_line_width(fw * 0.05)
    front_flap()
    ctx.stroke()
    ctx.restore()

    ctx.set_source_rgba(0, 0, 0, 0.18)
    ctx.set_line_width(fw * 0.015)
    front_flap()
    ctx.stroke()

    # Integrated Up-Badge
    if with_up_badge:
        bx = cx + fw*0.22
        by = fy + fh*0.52
        br = size * 0.22

        b_shad = cairo.RadialGradient(bx, by + br*0.2, br*0.4, bx, by + br*0.2, br*1.1)
        b_shad.add_color_stop_rgba(0, 0, 0, 0, 0.35)
        b_shad.add_color_stop_rgba(1, 0, 0, 0, 0.0)
        ctx.set_source(b_shad)
        ctx.arc(bx, by + br*0.2, br*1.1, 0, 2*math.pi)
        ctx.fill()

        b_pat = cairo.LinearGradient(bx, by - br, bx, by + br)
        b_pat.add_color_stop_rgb(0.0, *hex_to_rgb('#38BDF8'))
        b_pat.add_color_stop_rgb(1.0, *hex_to_rgb('#1D4ED8'))
        ctx.set_source(b_pat)
        ctx.arc(bx, by, br, 0, 2*math.pi)
        ctx.fill()

        ctx.set_source_rgba(1, 1, 1, 0.98)
        ctx.set_line_cap(cairo.LINE_CAP_ROUND)
        ctx.set_line_join(cairo.LINE_JOIN_ROUND)
        ctx.set_line_width(br * 0.32)
        ctx.move_to(bx - br*0.42, by + br*0.12)
        ctx.line_to(bx, by - br*0.38)
        ctx.line_to(bx + br*0.42, by + br*0.12)
        ctx.stroke()
        ctx.move_to(bx, by - br*0.38)
        ctx.line_to(bx, by + br*0.42)
        ctx.stroke()

    return surf

# =============================================================
# 11. ARCHIVE FILE ICON (7z / zip package)
# =============================================================
def render_archive(size=256):
    surf, ctx = create_surface(size)
    cx, cy = size/2.0, size/2.0

    bw = size * 0.74
    bh = size * 0.82
    bx = cx - bw/2.0
    by = cy - bh/2.0
    br = size * 0.16

    shadow_pat = cairo.RadialGradient(cx, cy + bh*0.32, bw*0.3, cx, cy + bh*0.32, bw*0.75)
    shadow_pat.add_color_stop_rgba(0, 0, 0, 0, 0.38)
    shadow_pat.add_color_stop_rgba(1, 0, 0, 0, 0.0)
    ctx.set_source(shadow_pat)
    ctx.arc(cx, cy + bh*0.32, bw*0.75, 0, 2*math.pi)
    ctx.fill()

    c_pat = cairo.LinearGradient(cx, by, cx, by + bh)
    c_pat.add_color_stop_rgb(0.0, *hex_to_rgb('#475569'))
    c_pat.add_color_stop_rgb(0.45, *hex_to_rgb('#1E293B'))
    c_pat.add_color_stop_rgb(1.0, *hex_to_rgb('#0F172A'))
    ctx.set_source(c_pat)
    round_rect(ctx, bx, by, bw, bh, br)
    ctx.fill()

    ctx.save()
    round_rect(ctx, bx, by, bw, bh, br)
    ctx.clip()

    # Compression bands
    band_pat = cairo.LinearGradient(bx, 0, bx + bw, 0)
    band_pat.add_color_stop_rgba(0.0, *hex_to_rgba('#06B6D4', 0.15))
    band_pat.add_color_stop_rgba(0.5, *hex_to_rgba('#22D3EE', 0.85))
    band_pat.add_color_stop_rgba(1.0, *hex_to_rgba('#06B6D4', 0.15))
    ctx.set_source(band_pat)
    round_rect(ctx, bx, by + bh*0.22, bw, bh*0.06, bh*0.02)
    ctx.fill()
    round_rect(ctx, bx, by + bh*0.72, bw, bh*0.06, bh*0.02)
    ctx.fill()

    hi = cairo.LinearGradient(cx, by, cx, by + bh*0.35)
    hi.add_color_stop_rgba(0, 1, 1, 1, 0.60)
    hi.add_color_stop_rgba(1, 1, 1, 1, 0.0)
    ctx.set_source(hi)
    ctx.set_line_width(bw * 0.04)
    round_rect(ctx, bx+1, by+1, bw-2, bh-2, br-1)
    ctx.stroke()
    ctx.restore()

    # Vertical Zipper track
    zw = bw * 0.16
    zx = cx - zw/2.0
    zy = by + bh * 0.08
    zh = bh * 0.84

    ctx.set_source_rgb(*hex_to_rgb('#020617'))
    round_rect(ctx, zx, zy, zw, zh, zw*0.3)
    ctx.fill()

    # Teeth
    ctx.set_source_rgb(*hex_to_rgb('#FDE68A'))
    ctx.set_line_width(size * 0.02)
    teeth_count = 11
    for i in range(teeth_count):
        ty = zy + (i + 0.5) * (zh / teeth_count)
        if i % 2 == 0:
            ctx.move_to(zx + zw*0.1, ty)
            ctx.line_to(zx + zw*0.65, ty)
        else:
            ctx.move_to(zx + zw*0.35, ty)
            ctx.line_to(zx + zw*0.9, ty)
        ctx.stroke()

    # Golden Zipper Slider & Pull Tab
    py = cy - bh*0.08
    pw = bw * 0.36
    ph = bh * 0.36
    px = cx - pw/2.0

    s_shad = cairo.RadialGradient(cx, py + ph*0.6, pw*0.3, cx, py + ph*0.6, pw*0.8)
    s_shad.add_color_stop_rgba(0, 0, 0, 0, 0.45)
    s_shad.add_color_stop_rgba(1, 0, 0, 0, 0.0)
    ctx.set_source(s_shad)
    ctx.arc(cx, py + ph*0.6, pw*0.8, 0, 2*math.pi)
    ctx.fill()

    g_pat = cairo.LinearGradient(cx, py, cx, py + ph)
    g_pat.add_color_stop_rgb(0.0, *hex_to_rgb('#FDE68A'))
    g_pat.add_color_stop_rgb(0.4, *hex_to_rgb('#F59E0B'))
    g_pat.add_color_stop_rgb(1.0, *hex_to_rgb('#B45309'))
    ctx.set_source(g_pat)
    round_rect(ctx, px, py, pw, ph, pw*0.24)
    ctx.fill()

    # Specular rim on pull tab
    ctx.save()
    round_rect(ctx, px, py, pw, ph, pw*0.24)
    ctx.clip()
    t_hi = cairo.LinearGradient(cx, py, cx, py + ph*0.35)
    t_hi.add_color_stop_rgba(0, 1, 1, 1, 0.75)
    t_hi.add_color_stop_rgba(1, 1, 1, 1, 0.0)
    ctx.set_source(t_hi)
    ctx.set_line_width(pw * 0.08)
    round_rect(ctx, px+1, py+1, pw-2, ph-2, pw*0.24 - 1)
    ctx.stroke()
    ctx.restore()

    # Stylized 7 on tab
    ctx.save()
    seven_top = py + ph * 0.22
    seven_bot = py + ph * 0.72
    ctx.set_line_width(pw * 0.16)
    ctx.set_line_cap(cairo.LINE_CAP_ROUND)
    ctx.set_line_join(cairo.LINE_JOIN_ROUND)
    # Shadow
    ctx.set_source_rgba(0, 0, 0, 0.3)
    ctx.move_to(cx - pw*0.22, seven_top + size*0.01)
    ctx.line_to(cx + pw*0.22, seven_top + size*0.01)
    ctx.line_to(cx - pw*0.04, seven_bot + size*0.01)
    ctx.stroke()
    # Glyph
    ctx.set_source_rgba(1, 1, 1, 0.98)
    ctx.move_to(cx - pw*0.22, seven_top)
    ctx.line_to(cx + pw*0.22, seven_top)
    ctx.line_to(cx - pw*0.04, seven_bot)
    ctx.stroke()
    ctx.restore()

    ctx.set_source_rgba(0, 0, 0, 0.22)
    ctx.set_line_width(bw * 0.015)
    round_rect(ctx, bx, by, bw, bh, br)
    ctx.stroke()

    return surf

# =============================================================
# 12. FILE DOCUMENT ICON
# =============================================================
def render_file(size=256):
    surf, ctx = create_surface(size)
    cx, cy = size/2.0, size/2.0

    dw = size * 0.66
    dh = size * 0.82
    dx = cx - dw/2.0
    dy = cy - dh/2.0
    dr = size * 0.10
    fold = dw * 0.32

    shadow_pat = cairo.RadialGradient(cx, dy + dh*0.8, dw*0.3, cx, dy + dh*0.8, dw*0.7)
    shadow_pat.add_color_stop_rgba(0, 0, 0, 0, 0.30)
    shadow_pat.add_color_stop_rgba(1, 0, 0, 0, 0.0)
    ctx.set_source(shadow_pat)
    ctx.arc(cx, dy + dh*0.8, dw*0.7, 0, 2*math.pi)
    ctx.fill()

    def sheet_path():
        ctx.new_sub_path()
        ctx.move_to(dx + dr, dy)
        ctx.line_to(dx + dw - fold, dy)
        ctx.line_to(dx + dw, dy + fold)
        ctx.line_to(dx + dw, dy + dh - dr)
        ctx.arc(dx + dw - dr, dy + dh - dr, dr, 0, math.pi/2)
        ctx.line_to(dx + dr, dy + dh)
        ctx.arc(dx + dr, dy + dh - dr, dr, math.pi/2, math.pi)
        ctx.line_to(dx, dy + dr)
        ctx.arc(dx + dr, dy + dr, dr, math.pi, 3*math.pi/2)
        ctx.close_path()

    s_pat = cairo.LinearGradient(dx, dy, dx, dy + dh)
    s_pat.add_color_stop_rgb(0.0, 1.0, 1.0, 1.0)
    s_pat.add_color_stop_rgb(0.7, *hex_to_rgb('#F8FAFC'))
    s_pat.add_color_stop_rgb(1.0, *hex_to_rgb('#E2E8F0'))
    ctx.set_source(s_pat)
    sheet_path()
    ctx.fill()

    def fold_path():
        ctx.new_sub_path()
        ctx.move_to(dx + dw - fold, dy)
        ctx.line_to(dx + dw - fold, dy + fold)
        ctx.line_to(dx + dw, dy + fold)
        ctx.close_path()

    ctx.save()
    ctx.set_source_rgba(0, 0, 0, 0.18)
    ctx.move_to(dx + dw - fold, dy + fold)
    ctx.line_to(dx + dw - fold - size*0.04, dy + fold + size*0.04)
    ctx.line_to(dx + dw + size*0.04, dy + fold + size*0.04)
    ctx.close_path()
    ctx.fill()

    f_pat = cairo.LinearGradient(dx + dw - fold, dy, dx + dw, dy + fold)
    f_pat.add_color_stop_rgb(0.0, *hex_to_rgb('#CBD5E1'))
    f_pat.add_color_stop_rgb(1.0, *hex_to_rgb('#94A3B8'))
    ctx.set_source(f_pat)
    fold_path()
    ctx.fill()
    ctx.restore()

    ctx.set_line_cap(cairo.LINE_CAP_ROUND)

    # Line 1: Sapphire blue
    l1_pat = cairo.LinearGradient(dx, 0, dx + dw, 0)
    l1_pat.add_color_stop_rgb(0.0, *hex_to_rgb('#3B82F6'))
    l1_pat.add_color_stop_rgb(1.0, *hex_to_rgb('#60A5FA'))
    ctx.set_source(l1_pat)
    ctx.set_line_width(dh * 0.08)
    ctx.move_to(dx + dw*0.18, dy + dh*0.38)
    ctx.line_to(dx + dw*0.78, dy + dh*0.38)
    ctx.stroke()

    # Line 2: Emerald mint
    l2_pat = cairo.LinearGradient(dx, 0, dx + dw, 0)
    l2_pat.add_color_stop_rgb(0.0, *hex_to_rgb('#10B981'))
    l2_pat.add_color_stop_rgb(1.0, *hex_to_rgb('#34D399'))
    ctx.set_source(l2_pat)
    ctx.set_line_width(dh * 0.07)
    ctx.move_to(dx + dw*0.18, dy + dh*0.55)
    ctx.line_to(dx + dw*0.68, dy + dh*0.55)
    ctx.stroke()

    # Line 3: Sunset amber
    l3_pat = cairo.LinearGradient(dx, 0, dx + dw, 0)
    l3_pat.add_color_stop_rgb(0.0, *hex_to_rgb('#F59E0B'))
    l3_pat.add_color_stop_rgb(1.0, *hex_to_rgb('#FBBF24'))
    ctx.set_source(l3_pat)
    ctx.set_line_width(dh * 0.07)
    ctx.move_to(dx + dw*0.18, dy + dh*0.72)
    ctx.line_to(dx + dw*0.52, dy + dh*0.72)
    ctx.stroke()

    ctx.set_source_rgba(0, 0, 0, 0.15)
    ctx.set_line_width(dw * 0.015)
    sheet_path()
    ctx.stroke()

    return surf

# =============================================================
# 13. WINDOW CONTROL BUTTONS: CLOSE, MINIMIZE, MAXIMIZE, RESTORE
# =============================================================
def render_window_button(kind='close', size=256):
    surf, ctx = create_surface(size)
    cx, cy, r = size / 2.0, size / 2.0, size * 0.42

    shadow_pat = cairo.RadialGradient(cx, cy + r*0.14, r*0.65, cx, cy + r*0.14, r*1.18)
    shadow_pat.add_color_stop_rgba(0, 0, 0, 0, 0.32)
    shadow_pat.add_color_stop_rgba(1, 0, 0, 0, 0.0)
    ctx.set_source(shadow_pat)
    ctx.arc(cx, cy + r*0.14, r*1.18, 0, 2*math.pi)
    ctx.fill()

    pat = cairo.LinearGradient(cx, cy - r, cx, cy + r)
    if kind == 'close':
        c1 = hex_to_rgb('#FF6559') # bright coral
        c2 = hex_to_rgb('#EF4444') # radiant red
        c3 = hex_to_rgb('#B91C1C') # deep crimson base
    elif kind == 'minimize':
        c1 = hex_to_rgb('#FCD34D') # bright amber gold
        c2 = hex_to_rgb('#F59E0B') # amber
        c3 = hex_to_rgb('#B45309') # deep warm amber
    elif kind == 'maximize':
        c1 = hex_to_rgb('#34D399') # bright mint emerald
        c2 = hex_to_rgb('#10B981') # vibrant emerald
        c3 = hex_to_rgb('#047857') # deep rich green
    elif kind == 'restore':
        c1 = hex_to_rgb('#2DD4BF') # cyan-teal
        c2 = hex_to_rgb('#0D9488') # deep teal
        c3 = hex_to_rgb('#0F766E')
    else:
        c1 = c2 = c3 = (0.5, 0.5, 0.5)

    pat.add_color_stop_rgb(0.0, *c1)
    pat.add_color_stop_rgb(0.65, *c2)
    pat.add_color_stop_rgb(1.0, *c3)
    ctx.set_source(pat)
    ctx.arc(cx, cy, r, 0, 2*math.pi)
    ctx.fill()

    ctx.save()
    ctx.arc(cx, cy, r, 0, 2*math.pi)
    ctx.clip()
    spec = cairo.LinearGradient(cx, cy - r, cx, cy)
    spec.add_color_stop_rgba(0.0, 1, 1, 1, 0.60)
    spec.add_color_stop_rgba(0.5, 1, 1, 1, 0.18)
    spec.add_color_stop_rgba(1.0, 1, 1, 1, 0.0)
    ctx.set_source(spec)
    ctx.set_line_width(r * 0.12)
    ctx.arc(cx, cy, r - r*0.06, 0, 2*math.pi)
    ctx.stroke()
    ctx.restore()

    ctx.set_source_rgba(0, 0, 0, 0.18)
    ctx.set_line_width(size * 0.015)
    ctx.arc(cx, cy, r, 0, 2*math.pi)
    ctx.stroke()

    ctx.save()
    s_off = size * 0.014

    if kind == 'close':
        arm = r * 0.40
        lw = r * 0.20
        ctx.set_line_cap(cairo.LINE_CAP_ROUND)
        ctx.set_line_width(lw)
        ctx.set_source_rgba(0, 0, 0, 0.26)
        ctx.move_to(cx - arm, cy - arm + s_off)
        ctx.line_to(cx + arm, cy + arm + s_off)
        ctx.move_to(cx + arm, cy - arm + s_off)
        ctx.line_to(cx - arm, cy + arm + s_off)
        ctx.stroke()
        ctx.set_source_rgba(1, 1, 1, 0.98)
        ctx.move_to(cx - arm, cy - arm)
        ctx.line_to(cx + arm, cy + arm)
        ctx.move_to(cx + arm, cy - arm)
        ctx.line_to(cx - arm, cy + arm)
        ctx.stroke()

    elif kind == 'minimize':
        arm = r * 0.46
        lw = r * 0.20
        ctx.set_line_cap(cairo.LINE_CAP_ROUND)
        ctx.set_line_width(lw)
        ctx.set_source_rgba(0, 0, 0, 0.26)
        ctx.move_to(cx - arm, cy + s_off)
        ctx.line_to(cx + arm, cy + s_off)
        ctx.stroke()
        ctx.set_source_rgba(1, 1, 1, 0.98)
        ctx.move_to(cx - arm, cy)
        ctx.line_to(cx + arm, cy)
        ctx.stroke()

    elif kind == 'maximize':
        bw = r * 0.72
        bh = r * 0.72
        lw = r * 0.18
        cr = r * 0.14
        ctx.set_line_width(lw)
        ctx.set_source_rgba(0, 0, 0, 0.26)
        round_rect(ctx, cx - bw/2, cy - bh/2 + s_off, bw, bh, cr)
        ctx.stroke()
        ctx.set_source_rgba(1, 1, 1, 0.98)
        round_rect(ctx, cx - bw/2, cy - bh/2, bw, bh, cr)
        ctx.stroke()

    elif kind == 'restore':
        bw = r * 0.54
        bh = r * 0.54
        lw = r * 0.16
        cr = r * 0.10
        ctx.set_line_width(lw)
        ctx.set_source_rgba(1, 1, 1, 0.75)
        round_rect(ctx, cx - bw*0.25, cy - bh*0.75, bw, bh, cr)
        ctx.stroke()
        ctx.set_source_rgba(0, 0, 0, 0.26)
        round_rect(ctx, cx - bw*0.75, cy - bh*0.25 + s_off, bw, bh, cr)
        ctx.stroke()
        ctx.set_source_rgba(1, 1, 1, 0.98)
        round_rect(ctx, cx - bw*0.75, cy - bh*0.25, bw, bh, cr)
        ctx.stroke()

    ctx.restore()
    return surf

# =============================================================
# 14. FLAGSHIP APP ICON (512x512)
# =============================================================
def draw_text_centered(ctx, text, cx, baseline, fsize, color,
                       weight=cairo.FONT_WEIGHT_BOLD, family="DejaVu Sans",
                       letter_spacing=0.0, shadow=0.0):
    """Draw horizontally centered text on a given baseline."""
    ctx.select_font_face(family, cairo.FONT_SLANT_NORMAL, weight)
    ctx.set_font_size(fsize)

    def advance(ch):
        return ctx.text_extents(ch).x_advance + letter_spacing

    total = sum(advance(ch) for ch in text) - letter_spacing
    x = cx - total / 2.0

    for pass_i in range(2 if shadow > 0 else 1):
        if pass_i == 0:
            ctx.set_source_rgba(0, 0, 0, shadow)
            dy = shadow * 0.55
        else:
            ctx.set_source_rgba(*color)
            dy = 0.0
        cx_pen = x
        for ch in text:
            ctx.move_to(cx_pen, baseline + dy)
            ctx.show_text(ch)
            cx_pen += advance(ch)


def text_block_height(ctx, fsize):
    ctx.select_font_face("DejaVu Sans", cairo.FONT_SLANT_NORMAL, cairo.FONT_WEIGHT_BOLD)
    ctx.set_font_size(fsize)
    ext = ctx.text_extents("zpy")
    return ext.height


def render_app_icon(size=512):
    surf, ctx = create_surface(size)
    cx, cy = size / 2.0, size / 2.0

    # --- Crimson squircle base -------------------------------------------
    bw = size * 0.88
    bh = size * 0.88
    bx = cx - bw / 2.0
    by = cy - bh / 2.0
    br = size * 0.22

    # Drop shadow
    shadow_pat = cairo.RadialGradient(cx, size * 0.92, bw * 0.3, cx, size * 0.92, bw * 0.75)
    shadow_pat.add_color_stop_rgba(0, 0, 0, 0, 0.45)
    shadow_pat.add_color_stop_rgba(1, 0, 0, 0, 0.0)
    ctx.set_source(shadow_pat)
    ctx.arc(cx, size * 0.92, bw * 0.75, 0, 2 * math.pi)
    ctx.fill()

    bg_pat = cairo.LinearGradient(bx, by, bx + bw, by + bh)
    bg_pat.add_color_stop_rgb(0.0, *hex_to_rgb('#6B0F26'))  # crimson
    bg_pat.add_color_stop_rgb(0.5, *hex_to_rgb('#310816'))  # dark wine
    bg_pat.add_color_stop_rgb(1.0, *hex_to_rgb('#0E0307'))  # near black
    ctx.set_source(bg_pat)
    round_rect(ctx, bx, by, bw, bh, br)
    ctx.fill()

    # Warm crimson ambience, top-left
    glow_pat = cairo.RadialGradient(bx + bw * 0.25, by + bh * 0.22, 0,
                                    bx + bw * 0.25, by + bh * 0.22, bw * 0.65)
    glow_pat.add_color_stop_rgba(0.0, *hex_to_rgba('#FB7185', 0.32))
    glow_pat.add_color_stop_rgba(1.0, *hex_to_rgba('#FB7185', 0.0))
    ctx.set_source(glow_pat)
    round_rect(ctx, bx, by, bw, bh, br)
    ctx.fill()

    # Luminous rim
    ctx.save()
    round_rect(ctx, bx, by, bw, bh, br)
    ctx.clip()
    rim_pat = cairo.LinearGradient(bx, by, bx, by + bh)
    rim_pat.add_color_stop_rgba(0.0, 1, 1, 1, 0.42)
    rim_pat.add_color_stop_rgba(0.3, *hex_to_rgba('#FB7185', 0.30))
    rim_pat.add_color_stop_rgba(1.0, 1, 1, 1, 0.05)
    ctx.set_source(rim_pat)
    ctx.set_line_width(size * 0.02)
    round_rect(ctx, bx + 1, by + 1, bw - 2, bh - 2, br - 1)
    ctx.stroke()
    ctx.restore()

    # --- Archive container (7-Zip style zipper body) ----------------------
    cw = size * 0.62
    ch = size * 0.66
    cx_box = cx - cw / 2.0
    cy_box = cy - ch / 2.0 + size * 0.02
    cr = size * 0.14

    c_shad = cairo.RadialGradient(cx, cy_box + ch * 0.8, cw * 0.3, cx, cy_box + ch * 0.8, cw * 0.8)
    c_shad.add_color_stop_rgba(0, 0, 0, 0, 0.65)
    c_shad.add_color_stop_rgba(1, 0, 0, 0, 0.0)
    ctx.set_source(c_shad)
    ctx.arc(cx, cy_box + ch * 0.8, cw * 0.8, 0, 2 * math.pi)
    ctx.fill()

    body_pat = cairo.LinearGradient(cx, cy_box, cx, cy_box + ch)
    body_pat.add_color_stop_rgb(0.0, *hex_to_rgb('#5A1B2B'))
    body_pat.add_color_stop_rgb(0.40, *hex_to_rgb('#2C0C17'))
    body_pat.add_color_stop_rgb(1.0, *hex_to_rgb('#0C0306'))
    ctx.set_source(body_pat)
    round_rect(ctx, cx_box, cy_box, cw, ch, cr)
    ctx.fill()

    ctx.save()
    round_rect(ctx, cx_box, cy_box, cw, ch, cr)
    ctx.clip()

    # Crimson glowing compression bands
    b1_pat = cairo.LinearGradient(cx_box, 0, cx_box + cw, 0)
    b1_pat.add_color_stop_rgba(0, *hex_to_rgba('#9F1239', 0.15))
    b1_pat.add_color_stop_rgba(0.5, *hex_to_rgba('#FB7185', 1.0))
    b1_pat.add_color_stop_rgba(1, *hex_to_rgba('#9F1239', 0.15))
    ctx.set_source(b1_pat)
    for band_y in (0.185, 0.745):
        round_rect(ctx, cx_box, cy_box + ch * band_y, cw, ch * 0.055, ch * 0.02)
        ctx.fill()
    # faint second line above each band for a machined look
    ctx.set_source_rgba(1.0, 0.85, 0.88, 0.10)
    for band_y in (0.165, 0.725):
        round_rect(ctx, cx_box, cy_box + ch * band_y, cw, ch * 0.012, ch * 0.006)
        ctx.fill()

    # vertical crimson sheen across the body
    body_sheen = cairo.LinearGradient(cx_box, 0, cx_box + cw, 0)
    body_sheen.add_color_stop_rgba(0.0, *hex_to_rgba('#FB7185', 0.0))
    body_sheen.add_color_stop_rgba(0.42, *hex_to_rgba('#FB7185', 0.10))
    body_sheen.add_color_stop_rgba(0.58, *hex_to_rgba('#FB7185', 0.10))
    body_sheen.add_color_stop_rgba(1.0, *hex_to_rgba('#FB7185', 0.0))
    ctx.set_source(body_sheen)
    round_rect(ctx, cx_box, cy_box, cw, ch, cr)
    ctx.fill()

    hi = cairo.LinearGradient(cx, cy_box, cx, cy_box + ch * 0.4)
    hi.add_color_stop_rgba(0, 1, 1, 1, 0.55)
    hi.add_color_stop_rgba(1, 1, 1, 1, 0.0)
    ctx.set_source(hi)
    ctx.set_line_width(cw * 0.05)
    round_rect(ctx, cx_box + 1, cy_box + 1, cw - 2, ch - 2, cr - 1)
    ctx.stroke()
    ctx.restore()

    # Vertical zipper track with metallic teeth
    zw = cw * 0.18
    zx = cx - zw / 2.0
    zy = cy_box + ch * 0.06
    zh = ch * 0.88

    ctx.set_source_rgb(*hex_to_rgb('#0A0305'))
    round_rect(ctx, zx, zy, zw, zh, zw * 0.3)
    ctx.fill()

    ctx.set_source_rgb(*hex_to_rgb('#FECDD3'))
    ctx.set_line_width(size * 0.016)
    teeth = 13
    for i in range(teeth):
        ty = zy + (i + 0.5) * (zh / teeth)
        if i % 2 == 0:
            ctx.move_to(zx + zw * 0.1, ty)
            ctx.line_to(zx + zw * 0.65, ty)
        else:
            ctx.move_to(zx + zw * 0.35, ty)
            ctx.line_to(zx + zw * 0.9, ty)
        ctx.stroke()

    # --- Crimson zipper pull tab with "rust" over "zip" -------------------
    pw = cw * 0.56
    ph = ch * 0.545
    px = cx - pw / 2.0
    py = cy - ph / 2.0 - size * 0.01
    pr = pw * 0.20

    p_shad = cairo.RadialGradient(cx, py + ph * 0.62, pw * 0.3, cx, py + ph * 0.62, pw * 0.95)
    p_shad.add_color_stop_rgba(0, 0, 0, 0, 0.60)
    p_shad.add_color_stop_rgba(1, 0, 0, 0, 0.0)
    ctx.set_source(p_shad)
    ctx.arc(cx, py + ph * 0.62, pw * 0.95, 0, 2 * math.pi)
    ctx.fill()

    # Brushed crimson metal
    g_pat = cairo.LinearGradient(cx, py, cx, py + ph)
    g_pat.add_color_stop_rgb(0.0, *hex_to_rgb('#FB7185'))
    g_pat.add_color_stop_rgb(0.22, *hex_to_rgb('#E11D48'))
    g_pat.add_color_stop_rgb(0.60, *hex_to_rgb('#9F1239'))
    g_pat.add_color_stop_rgb(1.0, *hex_to_rgb('#5C0A1C'))
    ctx.set_source(g_pat)
    round_rect(ctx, px, py, pw, ph, pr)
    ctx.fill()

    # Inner bevel: soft dark ring + top specular
    ctx.save()
    round_rect(ctx, px, py, pw, ph, pr)
    ctx.clip()
    t_hi = cairo.LinearGradient(cx, py, cx, py + ph * 0.34)
    t_hi.add_color_stop_rgba(0, 1, 1, 1, 0.50)
    t_hi.add_color_stop_rgba(1, 1, 1, 1, 0.0)
    ctx.set_source(t_hi)
    ctx.set_line_width(pw * 0.055)
    round_rect(ctx, px + 1, py + 1, pw - 2, ph - 2, pr - 1)
    ctx.stroke()

    # Engraved plate behind the lettering
    inset = pw * 0.10
    plate_top = py + ph * 0.12
    plate_h = ph * 0.62
    plate_pat = cairo.LinearGradient(cx, plate_top, cx, plate_top + plate_h)
    plate_pat.add_color_stop_rgba(0.0, *hex_to_rgba('#4C0519', 0.72))
    plate_pat.add_color_stop_rgba(1.0, *hex_to_rgba('#2B040D', 0.62))
    ctx.set_source(plate_pat)
    round_rect(ctx, px + inset, plate_top, pw - inset * 2, plate_h, pw * 0.09)
    ctx.fill()
    ctx.set_source_rgba(1, 1, 1, 0.18)
    ctx.set_line_width(size * 0.004)
    round_rect(ctx, px + inset, plate_top, pw - inset * 2, plate_h, pw * 0.09)
    ctx.stroke()
    ctx.restore()

    # Lettering: "rust" (small, airy) over "zip" (bold)
    rust_size = ph * 0.175
    zip_size = ph * 0.365
    rust_h = text_block_height(ctx, rust_size)
    zip_h = text_block_height(ctx, zip_size)
    gap = ph * 0.045
    block_h = rust_h + gap + zip_h
    block_top = plate_top + (plate_h - block_h) / 2.0

    rust_baseline = block_top + rust_size * 0.80
    zip_baseline = rust_baseline + gap + zip_h * 0.86

    draw_text_centered(ctx, "rust", cx, rust_baseline, rust_size,
                       (1.0, 0.93, 0.94, 0.95), letter_spacing=pw * 0.028,
                       shadow=0.55)
    draw_text_centered(ctx, "zip", cx, zip_baseline, zip_size,
                       (1.0, 1.0, 1.0, 0.99), letter_spacing=pw * 0.012,
                       shadow=0.55)

    # Specular sheen over the lettering plate
    ctx.save()
    round_rect(ctx, px, py, pw, ph, pr)
    ctx.clip()
    sheen = cairo.LinearGradient(px, py, px + pw * 0.7, py + ph)
    sheen.add_color_stop_rgba(0.0, 1, 1, 1, 0.20)
    sheen.add_color_stop_rgba(0.32, 1, 1, 1, 0.04)
    sheen.add_color_stop_rgba(1.0, 1, 1, 1, 0.0)
    ctx.set_source(sheen)
    round_rect(ctx, px, py, pw, ph, pr)
    ctx.fill()
    ctx.restore()

    # Pull tab outline
    ctx.set_source_rgba(0, 0, 0, 0.30)
    ctx.set_line_width(size * 0.006)
    round_rect(ctx, px, py, pw, ph, pr)
    ctx.stroke()

    # Eyelet hole
    ctx.set_source_rgb(*hex_to_rgb('#200508'))
    ctx.arc(cx, py + ph * 0.865, pw * 0.075, 0, 2 * math.pi)
    ctx.fill()
    ctx.set_source_rgba(1, 1, 1, 0.30)
    ctx.set_line_width(size * 0.004)
    ctx.arc(cx, py + ph * 0.865, pw * 0.075, math.pi, 2 * math.pi)
    ctx.stroke()

    # Container outline on top of everything
    ctx.set_source_rgba(0, 0, 0, 0.28)
    ctx.set_line_width(size * 0.005)
    round_rect(ctx, cx_box, cy_box, cw, ch, cr)
    ctx.stroke()

    return surf


def generate_all():
    assets_icons = 'assets/icons'
    assets_root = 'assets'

    print("Generating 2026-style icons...")

    # 1. Add
    print("  -> add.png")
    surf = render_add(256)
    save_icon(surf, f"{assets_icons}/add.png", 64)

    # 2. Extract
    print("  -> extract.png")
    surf = render_extract(256)
    save_icon(surf, f"{assets_icons}/extract.png", 64)

    # 3. Test
    print("  -> test.png")
    surf = render_test(256)
    save_icon(surf, f"{assets_icons}/test.png", 64)

    # 4. Copy
    print("  -> copy.png")
    surf = render_copy(256)
    save_icon(surf, f"{assets_icons}/copy.png", 64)

    # 5. Move
    print("  -> move.png")
    surf = render_move(256)
    save_icon(surf, f"{assets_icons}/move.png", 64)

    # 6. Delete
    print("  -> delete.png")
    surf = render_delete(256)
    save_icon(surf, f"{assets_icons}/delete.png", 64)

    # 7. Info
    print("  -> info.png")
    surf = render_info(256)
    save_icon(surf, f"{assets_icons}/info.png", 64)

    # 8. Up
    print("  -> up.png")
    surf = render_up(256)
    save_icon(surf, f"{assets_icons}/up.png", 64)

    # 9. Refresh
    print("  -> refresh.png")
    surf = render_refresh(256)
    save_icon(surf, f"{assets_icons}/refresh.png", 64)

    # 10. Folder
    print("  -> folder16.png & folder24.png")
    surf = render_folder(256, with_up_badge=False)
    save_icon(surf, f"{assets_icons}/folder16.png", 64)
    save_icon(surf, f"{assets_icons}/folder24.png", 64)

    # 11. Folder Up
    print("  -> folder_up16.png")
    surf = render_folder(256, with_up_badge=True)
    save_icon(surf, f"{assets_icons}/folder_up16.png", 64)

    # 12. Archive
    print("  -> archive16.png & archive24.png")
    surf = render_archive(256)
    save_icon(surf, f"{assets_icons}/archive16.png", 64)
    save_icon(surf, f"{assets_icons}/archive24.png", 64)

    # 13. File
    print("  -> file16.png")
    surf = render_file(256)
    save_icon(surf, f"{assets_icons}/file16.png", 64)

    # 14. Window controls: Close, Minimize (reduit), Maximize, Restore
    print("  -> window_close.png")
    surf = render_window_button('close', 256)
    save_icon(surf, f"{assets_icons}/window_close.png", 64)

    print("  -> window_minimize.png")
    surf = render_window_button('minimize', 256)
    save_icon(surf, f"{assets_icons}/window_minimize.png", 64)

    print("  -> window_maximize.png")
    surf = render_window_button('maximize', 256)
    save_icon(surf, f"{assets_icons}/window_maximize.png", 64)

    print("  -> window_restore.png")
    surf = render_window_button('restore', 256)
    save_icon(surf, f"{assets_icons}/window_restore.png", 64)

    # 15. Flagship App Icon
    print("  -> icon.png (256x256 and 512x512, plus 64px UI copy)")
    surf = render_app_icon(512)
    save_icon(surf, f"{assets_root}/icon.png", 256)
    save_icon(surf, f"{assets_root}/icon512.png", 512)
    # Small raster for the in-app titlebar: sampling a 256px texture down to
    # ~20px on the GPU aliases badly, so ship a pre-downsampled copy.
    save_icon(surf, f"{assets_root}/icon64.png", 64)

    print("All icons successfully generated!")

if __name__ == '__main__':
    generate_all()
