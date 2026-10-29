# Verevon Sans Lab

Parametric skeleton font. Each glyph is a centre-line stroked with an elliptical pen
(`stem_v` / `stem_h` in `params.py`), so weight is a number, not a redraw.
Original design; the Arbeit Pro trial was used only as a proportion reference (no outlines copied).

```bash
python3 build.py --install  # all weights -> out/*, and copy woffs into ../public/fonts/verevon-sans/
python3 -m http.server 8765 # then open specimen.html or sheet.html?w=300|400|700
```

Files: `params.py` (metrics/weights), `stroker.py` (pen offsetting), `kit.py` (arc/ring helpers),
`glyphs_lower.py`, `glyphs_caps.py`, `glyphs_misc.py` (digits, punctuation), `build.py`.

Kerning (`kern.py`) is computed from the outlines: ink gaps are sampled in horizontal bands and
compared with each glyph's gap to itself. It only tightens (never loosens), keeps a collision guard,
and punctuation pairs are hand-set rules. Compare on/off in `kern.html`.

Known gaps: no hinting, overlapping contours (non-zero fill, not merged), no `@ & €`,
no accented Latin beyond æøåÆØÅ, kerning is generated and has not been tuned by eye.
