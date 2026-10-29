"""Uppercase skeletons (plus Å Æ Ø)."""

from glyphs_lower import small_ring
from kit import Glyph, Kit
import json
from pathlib import Path

from stroker import Stroke, orient


def bowl_path(k: Kit, x_stem, y_top, y_bot, x_right_ink, rx):
    """Open-left bowl: horizontal in from the stem, round-off, horizontal back."""
    xr = x_right_ink - k.hv - rx
    cy = (y_top + y_bot) / 2
    return k.path((x_stem, y_top), k.arc(xr, cy, rx, (y_top - y_bot) / 2, 90, -90), (x_stem, y_bot))


def s_curve(k: Kit, W, y_mid, top_c, bot_c):
    ru = (top_c - y_mid) / 2
    rl = (y_mid - bot_c) / 2
    rx = W / 2 - k.hv
    return k.path(
        k.arc(W / 2, top_c - ru, rx, ru, 28, 238),
        k.arc(W / 2, bot_c + rl, rx, rl, 58, -152),
    )


MANROPE_V = json.loads((Path(__file__).parent / "manrope_v.json").read_text())


V_TOP_GAP = 1.03  # opening between the two arms at the top, relative to Manrope's V


def widen_v_top(poly, factor):
    """Spread the two arm tops apart so the top gap grows by `factor`; bottom points stay put."""
    top = sorted((p for p in poly if p[1] >= 699.0), key=lambda p: p[0])  # outerL, innerL, innerR, outerR
    d = (top[2][0] - top[1][0]) * (factor - 1.0) / 2
    mid = (top[1][0] + top[2][0]) / 2
    return [((x - d) if y >= 699.0 and x < mid else (x + d) if y >= 699.0 else x, y) for x, y in map(tuple, poly)]


def pointed_v(k: Kit, ink, tip_depth=1.0, tip_flat=95.0):
    """Capital V drawn as one polygon so the bottom comes to a point instead of a flat cut."""
    cap = k.cap
    cx = ink / 2
    y_tip = -tip_depth
    dy = cap - y_tip
    w2 = 2 * k.hx(cx, dy)  # horizontal thickness of a diagonal stroke
    y_inner = cap - (cx - w2) * dy / cx
    f = tip_flat / 2
    return orient(
        [
            (0, cap), (w2, cap), (cx, y_inner), (ink - w2, cap), (ink, cap),
            (cx + f, y_tip), (cx - f, y_tip),
        ],
        True,
    )


def build_a_cap(k: Kit, W=640):
    cap, hh = k.cap, k.hh
    xb_l = (W / 2) * 210 / cap
    return [
        k.line((0, 0), (W / 2, cap), cap0="h", cap1="h"),
        k.line((W, 0), (W / 2, cap), cap0="h", cap1="h"),
        k.line((xb_l, 210), (W - xb_l, 210)),
    ]


def build(k: Kit):
    cap, hv, hh, ov = k.cap, k.hv, k.hh, k.ov
    yt, yb = cap - hh, hh
    g = {}
    stem = k.line((hv, 0), (hv, cap))

    # --- straight-stem letters -------------------------------------------------
    g["I"] = Glyph([stem], sb="ss")
    g["H"] = Glyph([stem, k.line((590 - hv, 0), (590 - hv, cap)), k.line((hv, 350), (590 - hv, 350))], sb="ss")
    E_ARM = 470  # Arbeit Pro Regular E ink width; top +2%, bottom +3% on top of that
    E_TOP, E_BOT = round(E_ARM * 1.02), round(E_ARM * 1.03)
    E_MID, E_MID_Y = 429, 355  # Arbeit's own middle arm is ~9% shorter and sits 5 units above centre
    g["E"] = Glyph(
        [stem, *(k.line((hv, y), (x, y)) for y, x in ((yt, E_TOP), (E_MID_Y, E_MID), (yb, E_BOT)))],
        sb=(94, 65),
    )
    g["F"] = Glyph([stem, k.line((hv, yt), (520, yt)), k.line((hv, 320), (450, 320))], sb="sd")
    g["L"] = Glyph([stem, k.line((hv, yb), (470, yb))], sb="sd")
    g["T"] = Glyph([k.line((280, 0), (280, cap)), k.line((0, yt), (560, yt))], sb="dd")

    # --- bowls on a stem ---------------------------------------------------------
    ym_b = 0.54 * cap
    g["B"] = Glyph(
        [
            stem,
            bowl_path(k, hv, yt, ym_b, 520, 150),
            bowl_path(k, hv, ym_b, yb, 550, 165),
        ],
        sb="sr",
    )
    g["D"] = Glyph([stem, bowl_path(k, hv, yt, yb, 640, 260)], sb="sr")
    ym_p = 0.40 * cap
    g["P"] = Glyph([stem, bowl_path(k, hv, yt, ym_p, 570, 210)], sb="sr")
    ym_r = 0.47 * cap
    leg_foot_c = 543.0 - k.hx(543.0 - 290, ym_r)  # centre-line x so the flat-cut foot ends at ink x=543
    g["R"] = Glyph(
        [stem, bowl_path(k, hv, yt, ym_r, 515, 185), k.line((290, ym_r), (leg_foot_c, 0), cap1="h")],
        sb=(94, 39),
    )

    # --- rounds ------------------------------------------------------------------
    Wo = 644
    ovc, nc = k.p.overshoot_caps, k.p.squareness_caps
    g["O"] = Glyph([k.ring(0, Wo, -ovc, cap + ovc, n=nc)], sb=(29, 29))
    cy = cap / 2
    rx_c, ry_c = 315 - hv, (cap + 2 * ovc) / 2 - hh
    g["C"] = Glyph([Stroke(k.arc(315, cy, rx_c, ry_c, 40, 320, n=nc))], sb="rd")
    g["G"] = Glyph(
        [k.path(k.arc(330, cy, 330 - hv, ry_c, 40, 360, n=nc), (350, cy))],
        sb="rs",
    )
    g["Q"] = Glyph([k.ring(0, Wo, -ovc, cap + ovc, n=nc), k.line((410, 150), (595, -75))], sb=(29, 29))
    g["S"] = Glyph([s_curve(k, 520, 0.52 * cap, k.cap + ov - hh, -ov + hh)], sb="rr")

    Wu = 620
    ryu = Wu / 2 - hv - 20
    cyu = -ov + hh + ryu
    g["U"] = Glyph(
        [k.path((hv, cap), (hv, cyu), k.arc(Wu / 2, cyu, Wu / 2 - hv, ryu, 180, 360), (Wu - hv, cap))],
        sb="ss",
    )
    Wj = 380
    ryj = 190
    cyj = -ov + hh + ryj
    g["J"] = Glyph(
        [k.path((Wj - hv, cap), (Wj - hv, cyj), k.arc(Wj / 2, cyj, Wj / 2 - hv, ryj, 0, -165))],
        sb="rs",
    )

    # --- diagonals -----------------------------------------------------------------
    g["A"] = Glyph(build_a_cap(k), sb="dd")
    mv = MANROPE_V[str(k.p.weight_class)]  # Manrope's capital V (SIL OFL 1.1), scaled to our cap height
    g["V"] = Glyph([], [orient(widen_v_top(poly, V_TOP_GAP), True) for poly in mv["polygons"]], sb=(mv["lsb"], mv["rsb"]), exact=True)
    g["W"] = Glyph(
        [
            k.line((0, cap), (237, 0), cap0="h", cap1="h"),
            k.line((475, cap), (237, 0), cap0="h", cap1="h"),
            k.line((475, cap), (712, 0), cap0="h", cap1="h"),
            k.line((950, cap), (712, 0), cap0="h", cap1="h"),
        ],
        sb="dd",
    )
    Wm = 800
    g["M"] = Glyph(
        [
            stem,
            k.line((Wm - hv, 0), (Wm - hv, cap)),
            k.line((hv, cap), (Wm / 2, 0), cap0="h", cap1="h"),
            k.line((Wm - hv, cap), (Wm / 2, 0), cap0="h", cap1="h"),
        ],
        sb="ss",
    )
    Wn = 517  # stem-to-stem; flat-cut diagonal ends add ~12 to reach Manrope N ink width 529
    g["N"] = Glyph(
        [
            stem,
            k.line((Wn - hv, 0), (Wn - hv, cap)),
            k.line((hv, cap), (Wn - hv, 0), cap0="h", cap1="h"),
        ],
        sb=(68, 68),
    )
    g["X"] = Glyph(
        [
            k.line((0, cap), (640, 0), cap0="h", cap1="h"),
            k.line((640, cap), (0, 0), cap0="h", cap1="h"),
        ],
        sb="dd",
    )
    g["Y"] = Glyph(
        [
            k.line((0, cap), (310, 300), cap0="h"),
            k.line((620, cap), (310, 300), cap0="h"),
            k.line((310, 300), (310, 0)),
        ],
        sb="dd",
    )
    hz = k.hx(560 - 2 * k.hx(560, cap), cap)
    g["Z"] = Glyph(
        [
            k.line((0, yt), (560, yt)),
            k.line((560 - hz, cap), (hz, 0), cap0="h", cap1="h"),
            k.line((0, yb), (560, yb)),
        ],
        sb="dd",
    )
    g["K"] = Glyph(
        [
            stem,
            k.line((610, cap), (hv + 8, 240), cap0="h"),
            k.line((153, 330), (610, 0), cap1="h"),
        ],
        sb="sd",
    )

    # --- Norwegian -------------------------------------------------------------------
    g["Å"] = Glyph(build_a_cap(k) + [small_ring(k, 320, cap + 40 + 46 + hh)], sb="dd")
    g["Ø"] = Glyph([k.ring(0, Wo, -ovc, cap + ovc, n=nc), k.line((40, 35), (Wo - 40, cap - 35))], sb=(29, 29))

    xs = 400
    ym_ae = 330
    g["Æ"] = Glyph(
        [
            k.line((0, 0), (xs, cap), cap0="h", cap1="h"),
            k.line((xs, 0), (xs, cap)),
            k.line((xs, yt), (xs + 330, yt)),
            k.line((xs, yb), (xs + 340, yb)),
            k.line((xs * ym_ae / cap, ym_ae), (xs + 280, ym_ae)),
        ],
        sb="ds",
    )
    return g
