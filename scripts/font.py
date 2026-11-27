#!/usr/bin/env python3
"""Rasterize the bundled UI faces from system DejaVu fonts."""
from PIL import Image, ImageDraw, ImageFont

root = "/usr/share/fonts/truetype/dejavu"
for name, face, size in (
    ("regular", "DejaVuSans", 15),
    ("strong", "DejaVuSans-Bold", 20),
    ("display", "DejaVuSans-Bold", 44),
    ("mono", "DejaVuSansMono", 15),
):
    font = ImageFont.truetype(f"{root}/{face}.ttf", size)
    chars = range(32, 127)
    width = max(font.getbbox(chr(code))[2] for code in chars) + 3
    ascent, descent = font.getmetrics()
    height = ascent + descent
    print(f"static const unsigned char font_{name}[][{width * height}] = {{")
    for code in chars:
        glyph = Image.new("L", (width, height))
        ImageDraw.Draw(glyph).text((1, ascent), chr(code), font=font, fill=255, anchor="ls")
        pixels = [str((value + 8) // 17) for value in glyph.tobytes()]
        print("    { " + ",".join(pixels) + " },")
    print("};")
    widths = ", ".join(str(round(font.getlength(chr(code)))) for code in chars)
    print(f"static const unsigned char advance_{name}[] = {{ {widths} }};")
    print(f"static const struct Font {name} = {{ &font_{name}[0][0], advance_{name}, {width}, {height} }};")
