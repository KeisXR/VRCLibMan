"""Outline the "VRCLibMan" wordmark into an SVG of plain paths.

The font is only an input to this script and is not part of the repository or the app:
  npm pack @fontsource/urbanist   # Urbanist (SIL OFL 1.1), files/urbanist-latin-900-normal.woff2
  pip install fonttools brotli uharfbuzz
  python scripts/make-brand-wordmark.py urbanist-latin-900-normal.woff2 src/assets/brand/vrclibman-wordmark.svg
"""
import io
import sys

import uharfbuzz as hb
from fontTools.pens.svgPathPen import SVGPathPen
from fontTools.pens.transformPen import TransformPen
from fontTools.ttLib import TTFont

TEXT = "VRCLibMan"

font_path, out_path = sys.argv[1], sys.argv[2]
font = TTFont(font_path)
ttf = io.BytesIO()
font.flavor = None
font.save(ttf)
blob = hb.Blob(ttf.getvalue())
hb_font = hb.Font(hb.Face(blob))
buf = hb.Buffer()
buf.add_str(TEXT)
buf.guess_segment_properties()
hb.shape(hb_font, buf, {"kern": True, "liga": False})

glyph_set = font.getGlyphSet()
order = font.getGlyphOrder()
upm = font["head"].unitsPerEm
ascender = font["hhea"].ascent
descender = font["hhea"].descent

pen = SVGPathPen(glyph_set)
x = 0
for info, pos in zip(buf.glyph_infos, buf.glyph_positions):
    name = order[info.codepoint]
    # Font units are y-up; flip into SVG's y-down with the baseline at the ascender.
    glyph_set[name].draw(TransformPen(pen, (1, 0, 0, -1, x + pos.x_offset, ascender - pos.y_offset)))
    x += pos.x_advance

# Trim the viewBox to the ink so the image lines up with the icon next to it.
from fontTools.pens.boundsPen import BoundsPen

bounds = BoundsPen(glyph_set)
x = 0
for info, pos in zip(buf.glyph_infos, buf.glyph_positions):
    glyph_set[order[info.codepoint]].draw(TransformPen(bounds, (1, 0, 0, -1, x + pos.x_offset, ascender - pos.y_offset)))
    x += pos.x_advance
x0, y0, x1, y1 = bounds.bounds
width, height = x1 - x0, y1 - y0
svg = (
    f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="{x0:.0f} {y0:.0f} {width:.0f} {height:.0f}" '
    f'width="{width / upm * 100:.2f}" height="{height / upm * 100:.2f}" fill="currentColor" '
    f'role="img" aria-label="{TEXT}">'
    f"<title>{TEXT}</title>"
    "<!-- Outlined from Urbanist Black 900 (SIL OFL 1.1) by scripts/make-brand-wordmark.py. "
    "Glyph outlines only; no font file is embedded. -->"
    f'<path d="{pen.getCommands()}"/></svg>\n'
)
with open(out_path, "w", encoding="utf-8") as handle:
    handle.write(svg)
print(out_path, f"{width:.0f}x{height:.0f} units", f"aspect {width / height:.3f}")
