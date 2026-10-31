"""Build Verevon Sans Lab fonts from the parametric skeletons.

    python3 build.py            # all weights
    python3 build.py light      # one weight
    python3 build.py --install  # also copy woffs into ../public/fonts/verevon-sans/

Writes out/VerevonSansLab-<Style>.ttf and .woff.
"""

import sys
from pathlib import Path

from fontTools.agl import AGL2UV, UV2AGL
from fontTools.feaLib.builder import addOpenTypeFeaturesFromString
from fontTools.fontBuilder import FontBuilder
from fontTools.pens.ttGlyphPen import TTGlyphPen

import glyphs_caps
import kern
import glyphs_lower
import glyphs_misc
from kit import Kit
from params import WEIGHTS, Params
from stroker import stroke_to_polygons

OUT = Path(__file__).parent / "out"
FAMILY = "Verevon Sans Lab"
ALIASES = {0x2019: "quotesingle", 0x2018: "quotesingle", 0x201C: "quotedbl", 0x201D: "quotedbl"}


def glyph_polygons(glyph, k: Kit):
    polys = []
    for s in glyph.strokes:
        polys.extend(stroke_to_polygons(s, k.hv, k.hh))
    polys.extend(glyph.fills)
    return polys


def collect(p: Params):
    k = Kit(p)
    raw = {}
    raw.update(glyphs_lower.build(k))
    raw.update(glyphs_caps.build(k))
    raw.update(glyphs_misc.build(k))
    side = {"s": p.sb_s, "r": p.sb_r, "d": p.sb_d}

    out = {}  # key -> (polygons, advance)
    for key, glyph in raw.items():
        polys = glyph_polygons(glyph, k)
        if not polys:
            out[key] = ([], round(260 * p.width_scale) if key == "space" else 0)
            continue
        xs = [x for poly in polys for x, _ in poly]
        xmin, xmax = min(xs), max(xs)
        if key.isdigit():
            adv = p.tabular_digit_advance
            dx = (adv - (xmax - xmin)) / 2 - xmin
        else:
            lsb, rsb = glyph.sb if isinstance(glyph.sb, tuple) else (side[glyph.sb[0]], side[glyph.sb[1]])
            dx = lsb - xmin
            adv = round(xmax - xmin + lsb + rsb)
        ws = p.longer if glyph.exact else p.width_scale
        hs = p.height_scale
        out[key] = ([[((x + dx) * ws, y * hs) for x, y in poly] for poly in polys], round(adv * ws))
    return out


def codepoint(key):
    return ord(key) if len(key) == 1 else AGL2UV[key]


def draw(polys):
    pen = TTGlyphPen(None)
    for poly in polys:
        pts = [(round(x), round(y)) for x, y in poly]
        dedup = [pt for i, pt in enumerate(pts) if i == 0 or pt != pts[i - 1]]
        if len(dedup) > 1 and dedup[0] == dedup[-1]:
            dedup.pop()
        if len(dedup) < 3:
            continue
        pen.moveTo(dedup[0])
        for pt in dedup[1:]:
            pen.lineTo(pt)
        pen.closePath()
    return pen.glyph()


def notdef(p: Params):
    outer = [(60, 0), (60, 700), (460, 700), (460, 0)]
    inner = [(120, 60), (400, 60), (400, 640), (120, 640)]
    return [outer, inner], 520


def build_font(p: Params, path_stem: Path):
    data = collect(p)
    order = [".notdef"]
    cmap = {}
    glyphs = {}
    metrics = {}

    nd_polys, nd_adv = notdef(p)
    glyphs[".notdef"] = draw(nd_polys)
    metrics[".notdef"] = (nd_adv, 60)

    for key, (polys, adv) in sorted(data.items(), key=lambda kv: codepoint(kv[0])):
        cp = codepoint(key)
        name = UV2AGL.get(cp, f"uni{cp:04X}")
        order.append(name)
        cmap[cp] = name
        glyphs[name] = draw(polys)
        xs = [round(x) for poly in polys for x, _ in poly]
        metrics[name] = (adv, min(xs) if xs else 0)

    for cp, key in ALIASES.items():
        cmap[cp] = UV2AGL[codepoint(key)]

    fb = FontBuilder(p.upm, isTTF=True)
    fb.setupGlyphOrder(order)
    fb.setupCharacterMap(cmap)
    fb.setupGlyf(glyphs)
    fb.setupHorizontalMetrics(metrics)
    hs = p.height_scale
    fb.setupHorizontalHeader(ascent=round(960 * hs), descent=-round(260 * hs))
    fb.setupNameTable(
        {
            "familyName": FAMILY,
            "styleName": p.style_name,
            "uniqueFontIdentifier": f"VerevonSansLab-{p.style_name}-0.1",
            "fullName": f"{FAMILY} {p.style_name}",
            "psName": f"VerevonSansLab-{p.style_name}",
            "version": "Version 0.1",
            "copyright": "Skeleton-generated design. Capital V outline from Manrope, Copyright 2018 The Manrope Project Authors (https://github.com/googlefonts/manrope), SIL Open Font License 1.1.",
            "licenseDescription": "This font includes glyph data from Manrope and is licensed under the SIL Open Font License, Version 1.1.",
            "licenseInfoURL": "https://openfontlicense.org",
        }
    )
    fb.setupOS2(
        sTypoAscender=round(800 * hs),
        sTypoDescender=-round(220 * hs),
        sTypoLineGap=0,
        usWinAscent=round(960 * hs),
        usWinDescent=round(260 * hs),
        sxHeight=round(p.x_height * hs),
        sCapHeight=round(p.cap_height * hs),
        usWeightClass=p.weight_class,
        version=4,
        fsSelection=0x40 | 0x80,
        achVendID="VRVN",
    )
    fb.setupPost()
    pairs = kern.compute(data, round(p.cap_height * hs), round(p.x_height * hs))
    rules = "\n".join(
        f"    pos {UV2AGL[codepoint(a)]} {UV2AGL[codepoint(b)]} {v};" for (a, b), v in sorted(pairs.items())
    )
    addOpenTypeFeaturesFromString(fb.font, f"languagesystem DFLT dflt;\nlanguagesystem latn dflt;\nfeature kern {{\n{rules}\n}} kern;\n")
    print(f"  kerning pairs: {len(pairs)}")
    fb.save(str(path_stem) + ".ttf")
    fb.font.flavor = "woff"
    fb.font.save(str(path_stem) + ".woff")
    return len(order)


SITE_FONTS = Path(__file__).parent.parent / "public" / "fonts" / "verevon-sans"


def install(p: Params):
    """Copy the woff for a weight into verevon-web/public as verevon-sans-<weight>.woff."""
    SITE_FONTS.mkdir(parents=True, exist_ok=True)
    src = OUT / f"VerevonSansLab-{p.style_name}.woff"
    (SITE_FONTS / f"verevon-sans-{p.weight_class}.woff").write_bytes(src.read_bytes())


def main(argv):
    OUT.mkdir(exist_ok=True)
    do_install = "--install" in argv
    argv = [a for a in argv if a != "--install"]
    names = argv or list(WEIGHTS)
    for name in names:
        p = WEIGHTS[name]
        n = build_font(p, OUT / f"VerevonSansLab-{p.style_name}")
        print(f"{p.style_name}: {n} glyphs -> out/VerevonSansLab-{p.style_name}.ttf")
        if do_install:
            install(p)


if __name__ == "__main__":
    main(sys.argv[1:])
