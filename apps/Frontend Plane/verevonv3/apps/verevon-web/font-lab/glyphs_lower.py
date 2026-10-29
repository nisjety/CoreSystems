"""Lowercase skeletons (plus å æ ø). Coordinates are local; ink is re-centred at build time."""

from kit import Glyph, Kit, translate, translate_fills
from stroker import Stroke


def small_ring(k: Kit, cx, cy):
    return k.ring(cx - (46 + k.hv), cx + (46 + k.hv), cy - (46 + k.hh), cy + (46 + k.hh))


def build_a(k: Kit, W=440, stem_bottom=0.0):
    ry = 150
    cy_s = k.round_top_c - ry
    shoulder = k.path(k.arc(W / 2, cy_s, W / 2 - k.hv, ry, 155, 0), (W - k.hv, stem_bottom))
    bowl = k.ring(0, W, -k.ov, 0.57 * k.xh)
    return [shoulder, bowl]


def build_e(k: Kit, W=480, x0=0.0):
    cy = k.xh / 2
    ring_pts = k.arc(x0 + W / 2, cy, W / 2 - k.hv, (k.xh + 2 * k.ov) / 2 - k.hh, 0, 322)
    bar = k.line((x0 + k.hv, cy), (x0 + W - k.hv, cy))
    return [Stroke(ring_pts), bar]


def arch(k: Kit, x_left, x_right, ya):
    """Shoulder of n/m/h/r: from stem centre-line x_left over to x_right."""
    rx = (x_right - x_left) / 2
    ry = k.round_top_c - ya
    return k.arc(x_left + rx, ya, rx, ry, 180, 0)


def build(k: Kit):
    xh, asc, desc, ov, hv, hh = k.xh, k.asc, k.desc, k.ov, k.hv, k.hh
    g = {}
    ya = 0.56 * xh

    # --- round family -------------------------------------------------------
    Wo = 490
    g["o"] = Glyph([k.ring(0, Wo, -ov, xh + ov)], sb="rr")
    g["c"] = Glyph(
        [Stroke(k.arc(225, xh / 2, 225 - hv + 4, (xh + 2 * ov) / 2 - hh, 42, 318))], sb="rd"
    )
    g["e"] = Glyph(build_e(k), sb="rr")

    Wd = 470
    bowl = k.ring(0, Wd, -ov, xh + ov)
    g["d"] = Glyph([bowl, k.line((Wd - hv, 0), (Wd - hv, asc))], sb="rs")
    g["b"] = Glyph([bowl, k.line((hv, 0), (hv, asc))], sb="sr")
    g["p"] = Glyph([bowl, k.line((hv, xh), (hv, desc))], sb="sr")
    g["q"] = Glyph([bowl, k.line((Wd - hv, xh), (Wd - hv, desc))], sb="rs")

    ryh = 130
    yh = desc + 10 + hh + ryh
    g["g"] = Glyph(
        [
            bowl,
            k.path((Wd - hv, xh), (Wd - hv, yh), k.arc(Wd / 2, yh, Wd / 2 - hv, ryh, 0, -158)),
        ],
        sb="rs",
    )

    g["a"] = Glyph(build_a(k), sb="rs")

    # --- arches -------------------------------------------------------------
    Wn = 420
    n_strokes = [
        k.line((hv, 0), (hv, xh)),
        k.path(arch(k, hv, Wn - hv, ya), (Wn - hv, 0)),
    ]
    g["n"] = Glyph(n_strokes, sb="ss")
    g["h"] = Glyph([k.line((hv, 0), (hv, asc)), n_strokes[1]], sb="ss")

    Wm = 690
    mid = Wm / 2
    g["m"] = Glyph(
        [
            k.line((hv, 0), (hv, xh)),
            k.path(arch(k, hv, mid, ya), (mid, 0)),
            k.path(arch(k, mid, Wm - hv, ya), (Wm - hv, 0)),
        ],
        sb="ss",
    )

    yb = xh - ya
    ryb = yb + ov - hh
    g["u"] = Glyph(
        [
            k.path((hv, xh), (hv, yb), k.arc(Wn / 2, yb, Wn / 2 - hv, ryb, 180, 360), (Wn - hv, xh)),
            k.line((Wn - hv, 0), (Wn - hv, xh)),
        ],
        sb="ss",
    )

    rr = 175
    g["r"] = Glyph(
        [
            k.line((hv, 0), (hv, xh)),
            k.path(k.arc(hv + rr, ya, rr, k.round_top_c - ya, 180, 52)),
        ],
        sb="sd",
    )

    # --- stems & terminals --------------------------------------------------
    g["i"] = Glyph([k.line((hv, 0), (hv, xh))], [k.dot(hv, xh + 118)], sb="ss")
    g["l"] = Glyph([k.line((hv, 0), (hv, asc))], sb="ss")

    xj = hv + 100
    ryj = 110
    yj = desc + 10 + hh + ryj
    g["j"] = Glyph(
        [k.path((xj, xh), (xj, yj), k.arc(xj - 100, yj, 100, ryj, 0, -125))],
        [k.dot(xj, xh + 118)],
        sb="ds",
    )

    xf = 125
    g["f"] = Glyph(
        [
            k.path((xf, 0), (xf, asc - hh - 135), k.arc(xf + 135, asc - hh - 135, 135, 135, 180, 62)),
            k.line((0, k.top_c), (xf + 205, k.top_c)),
        ],
        sb="dd",
    )

    xt = 128
    ryt = 105
    g["t"] = Glyph(
        [
            k.path(
                (xt, 650),
                (xt, k.round_bot_c + ryt),
                k.arc(xt + 120, k.round_bot_c + ryt, 120, ryt, 180, 292),
            ),
            k.line((0, k.top_c), (xt + 190, k.top_c)),
        ],
        sb="dd",
    )

    # --- s --------------------------------------------------------------------
    Ws = 400
    ym = xh / 2
    ru = (k.round_top_c - ym) / 2
    rl = (ym - k.round_bot_c) / 2
    rx = Ws / 2 - hv
    g["s"] = Glyph(
        [
            k.path(
                k.arc(Ws / 2, k.round_top_c - ru, rx, ru, 28, 238),
                k.arc(Ws / 2, k.round_bot_c + rl, rx, rl, 58, -152),
            )
        ],
        sb="rr",
    )

    # --- diagonals (flat-cut at baseline / x-height) ---------------------------
    Wv = 480
    g["v"] = Glyph(
        [
            k.line((0, xh), (Wv / 2, 0), cap0="h", cap1="h"),
            k.line((Wv, xh), (Wv / 2, 0), cap0="h", cap1="h"),
        ],
        sb="dd",
    )
    g["w"] = Glyph(
        [
            k.line((0, xh), (165, 0), cap0="h", cap1="h"),
            k.line((330, xh), (165, 0), cap0="h", cap1="h"),
            k.line((330, xh), (495, 0), cap0="h", cap1="h"),
            k.line((660, xh), (495, 0), cap0="h", cap1="h"),
        ],
        sb="dd",
    )
    g["x"] = Glyph(
        [
            k.line((0, xh), (470, 0), cap0="h", cap1="h"),
            k.line((470, xh), (0, 0), cap0="h", cap1="h"),
        ],
        sb="dd",
    )
    Wy = 480
    g["y"] = Glyph(
        [
            k.line((Wy, xh), (Wy * 0.2, desc), cap0="h", cap1="h"),
            k.line((0, xh), (Wy * 0.55, 90), cap0="h"),
        ],
        sb="dd",
    )
    Wz = 430
    hz = k.hx(Wz - 2 * k.hx(Wz, xh), xh)
    g["z"] = Glyph(
        [
            k.line((0, k.top_c), (Wz, k.top_c)),
            k.line((Wz - hz, xh), (hz, 0), cap0="h", cap1="h"),
            k.line((0, hh), (Wz, hh)),
        ],
        sb="dd",
    )
    Wk = 440
    g["k"] = Glyph(
        [
            k.line((hv, 0), (hv, asc)),
            k.line((Wk - 6, xh), (hv + 8, 190), cap0="h"),
            k.line((hv + 120, 272), (Wk - 6, 0), cap1="h"),
        ],
        sb="sd",
    )

    # --- Norwegian ------------------------------------------------------------
    a = build_a(k)
    a_cx = 220
    g["å"] = Glyph(
        a + [small_ring(k, a_cx, xh + ov + 55 + 46 + hh)],
        sb="rs",
    )
    g["ø"] = Glyph(
        [k.ring(0, Wo, -ov, xh + ov), k.line((50, 30), (Wo - 50, xh - 30))],
        sb="rr",
    )

    Wa = 400
    a_part = build_a(k, W=Wa, stem_bottom=xh * 0.5)
    e_part = build_e(k, W=470, x0=Wa - 2 * hv)
    g["æ"] = Glyph(a_part + e_part, sb="rr")
    return g
