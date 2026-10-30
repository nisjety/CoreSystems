"""One-off: extract Manrope's capital V (SIL OFL 1.1) at three weights into manrope_v.json.

Needs fonttools + brotli (reads the woff2). Run once; build.py only reads the JSON.
    python extract_manrope_v.py ../public/fonts/manrope/manrope-latin-variable.woff2
"""

import json
import sys

from fontTools.pens.recordingPen import DecomposingRecordingPen
from fontTools.ttLib import TTFont
from fontTools.varLib import instancer

TARGET_CAP = 700
WEIGHTS = {300: "light", 400: "regular", 700: "bold"}


def polygon_from_ops(ops):
    contours, cur = [], []
    for op, args in ops:
        if op == "moveTo":
            cur = [args[0]]
        elif op == "lineTo":
            cur.append(args[0])
        elif op in ("qCurveTo", "curveTo"):
            raise SystemExit(f"V contains curves ({op}); straight-line import only")
        elif op in ("closePath", "endPath"):
            contours.append(cur)
            cur = []
    return contours


def main(path):
    out = {}
    for wght in WEIGHTS:
        font = instancer.instantiateVariableFont(TTFont(path), {"wght": wght}, inplace=False)
        gs = font.getGlyphSet()
        name = font.getBestCmap()[ord("V")]
        pen = DecomposingRecordingPen(gs)
        gs[name].draw(pen)
        cap = font["OS/2"].sCapHeight
        s = TARGET_CAP / cap
        adv = font["hmtx"][name][0]
        polys = [[(round(x * s, 2), round(y * s, 2)) for x, y in c] for c in polygon_from_ops(pen.value)]
        xs = [x for p in polys for x, _ in p]
        out[str(wght)] = {
            "polygons": polys,
            "lsb": round(min(xs), 2),
            "rsb": round(adv * s - max(xs), 2),
        }
        print(wght, "cap", cap, "scale", round(s, 4), "contours", len(polys), "points", [len(p) for p in polys],
              "ink", round(max(xs) - min(xs), 1), "lsb", out[str(wght)]["lsb"], "rsb", out[str(wght)]["rsb"])
    json.dump(out, open("manrope_v.json", "w"), indent=1)


if __name__ == "__main__":
    main(sys.argv[1])
