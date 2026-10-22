#!/usr/bin/env python3
"""Renders app/icons/* from code (requires Pillow). Run: python3 scripts/make_icons.py"""
from pathlib import Path
from PIL import Image, ImageDraw

OUT = Path(__file__).resolve().parent.parent / "app" / "icons"
S = 1024  # supersampled canvas

def gradient(size, c1, c2):
    g = Image.new("RGBA", (size, size))
    px = g.load()
    for y in range(size):
        for x in range(size):
            t = (x + (size - y)) / (2 * size)
            px[x, y] = tuple(int(c1[i] + (c2[i] - c1[i]) * t) for i in range(3)) + (255,)
    return g

def key_layer(size, color=None):
    """Key shape (bow ring + blade + teeth), rotated 45 degrees."""
    mask = Image.new("L", (size, size), 0)
    d = ImageDraw.Draw(mask)
    k = size / 512
    cx, cy, r, w = 200 * k, 256 * k, 64 * k, 32 * k
    d.ellipse([cx - r - w / 2, cy - r - w / 2, cx + r + w / 2, cy + r + w / 2], fill=255)
    d.ellipse([cx - r + w / 2, cy - r + w / 2, cx + r - w / 2, cy + r - w / 2], fill=0)
    d.rounded_rectangle([250 * k, 240 * k, 390 * k, 272 * k], radius=16 * k, fill=255)
    d.rounded_rectangle([320 * k, 262 * k, 340 * k, 300 * k], radius=8 * k, fill=255)
    d.rounded_rectangle([358 * k, 262 * k, 378 * k, 316 * k], radius=8 * k, fill=255)
    return mask.rotate(-45, resample=Image.BICUBIC, center=(size / 2, size / 2))

def app_icon():
    img = Image.new("RGBA", (S, S), (0, 0, 0, 0))
    bg = Image.new("L", (S, S), 0)
    ImageDraw.Draw(bg).rounded_rectangle([64 * 2, 64 * 2, 448 * 2, 448 * 2], radius=96 * 2, fill=255)
    img.paste((15, 23, 42, 255), (0, 0), bg)
    grad = gradient(S // 4, (99, 102, 241), (139, 92, 246)).resize((S, S))
    img.paste(grad, (0, 0), key_layer(S))
    return img

def tray_icon():
    """Simple white-on-transparent key, readable on light and dark bars."""
    img = Image.new("RGBA", (S, S), (0, 0, 0, 0))
    mask = key_layer(int(S * 1.25)).crop((S // 8, S // 8, S // 8 + S, S // 8 + S))
    img.paste((99, 102, 241, 255), (0, 0), mask)
    return img

OUT.mkdir(parents=True, exist_ok=True)
icon = app_icon()
for name, size in [("32x32.png", 32), ("128x128.png", 128), ("128x128@2x.png", 256), ("icon.png", 512)]:
    icon.resize((size, size), Image.LANCZOS).save(OUT / name)
icon.resize((1024, 1024), Image.LANCZOS).save(OUT / "icon.icns")
icon.resize((256, 256), Image.LANCZOS).save(OUT / "icon.ico", sizes=[(16, 16), (24, 24), (32, 32), (48, 48), (64, 64), (128, 128), (256, 256)])
tray_icon().resize((64, 64), Image.LANCZOS).save(OUT / "tray.png")
print("icons written to", OUT)
