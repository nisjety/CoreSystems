"""Digits and punctuation."""

from kit import Glyph, Kit, rot180
from stroker import Stroke


def build_six(k: Kit, W=460):
    hv = k.hv
    cy_bowl = (-k.ov + 410) / 2
    ry_hook = k.cap + k.ov - k.hh - 470
    return [
        k.ring(0, W, -k.ov, 410),
        k.path((hv, cy_bowl), (hv, 470), k.arc(hv + (W - 2 * hv) * 0.62, 470, (W - 2 * hv) * 0.62, ry_hook, 180, 50)),
    ]


def build(k: Kit):
    cap, xh, hv, hh, ov = k.cap, k.xh, k.hv, k.hh, k.ov
    yt, yb = cap - hh, hh
    rd = hv + 9
    g = {}

    # --- digits (tabular: centred in a fixed advance at build time) ---------------
    g["0"] = Glyph([k.ring(0, 470, -ov, cap + ov)])

    g["1"] = Glyph(
        [k.line((60, 585), (215, cap), cap1="h"), k.line((215, 0), (215, cap))],
    )

    W2 = 450
    rx2 = W2 / 2 - hv
    ry2 = 190
    arc2 = k.arc(W2 / 2, k.cap + ov - hh - ry2, rx2, ry2, 158, -38)
    foot = 28.0
    for _ in range(3):
        foot = k.hx(arc2[-1][0] - foot, arc2[-1][1])
    g["2"] = Glyph(
        [
            k.path(arc2, (foot, 0), cap1="h"),
            k.line((0, yb), (W2, yb)),
        ]
    )

    W3 = 440
    ym3 = 0.55 * cap
    ru3 = (cap + ov - hh - ym3) / 2
    rl3 = (ym3 - (-ov + hh)) / 2
    rx3 = W3 / 2 - hv
    g["3"] = Glyph(
        [
            k.path(
                k.arc(W3 / 2 - 6, cap + ov - hh - ru3, rx3 - 12, ru3, 155, -90),
                (W3 / 2 - 70, ym3),
            ),
            k.path(
                (W3 / 2 - 70, ym3),
                k.arc(W3 / 2, -ov + hh + rl3, rx3, rl3, 90, -158),
            ),
        ]
    )

    W4 = 480
    g["4"] = Glyph(
        [
            k.line((0.72 * W4 + hv - k.hx(0.72 * W4, cap - 215), cap), (0, 215), cap0="h"),
            k.line((0, 200), (W4, 200)),
            k.line((0.72 * W4, 0), (0.72 * W4, cap)),
        ]
    )

    W5 = 450
    bowl = k.arc(W5 / 2, (410 - ov) / 2, W5 / 2 - hv, (410 + ov) / 2 - hh, 140, -152)
    p0 = bowl[0]
    sx = p0[0]
    g["5"] = Glyph(
        [
            k.path((W5 - 10, yt), (sx, yt), (sx, p0[1] - 34)),
            Stroke(bowl),
        ]
    )

    six = build_six(k)
    g["6"] = Glyph(six)
    g["9"] = Glyph(rot180(six, 230, cap / 2))

    h7 = k.hx(470 - 2 * k.hx(340, cap), cap)
    g["7"] = Glyph(
        [k.line((0, yt), (470, yt)), k.line((470 - h7, cap), (130, 0), cap0="h", cap1="h")]
    )
    g["8"] = Glyph([k.ring(28, 432, 340, cap + ov), k.ring(0, 460, -ov, 385)])

    # --- punctuation ----------------------------------------------------------------
    g["period"] = Glyph([], [k.dot(rd, rd)], sb="ss")
    g["comma"] = Glyph([k.line((rd + 6, rd), (rd - 26, -112))], [k.dot(rd, rd)], sb="ss")
    g["colon"] = Glyph([], [k.dot(rd, rd), k.dot(rd, xh - rd)], sb="ss")
    g["semicolon"] = Glyph(
        [k.line((rd + 6, rd), (rd - 26, -112))], [k.dot(rd, rd), k.dot(rd, xh - rd)], sb="ss"
    )
    g["exclam"] = Glyph([k.line((hv, cap), (hv, 235))], [k.dot(hv, rd)], sb="ss")
    Wq = 360
    ryq = 170
    qarc = k.arc(Wq / 2, k.cap + ov - hh - ryq, Wq / 2 - hv, ryq, 160, -78)
    g["question"] = Glyph([k.path(qarc, (Wq / 2, 245))], [k.dot(Wq / 2, rd)], sb="rd")
    g["hyphen"] = Glyph([k.line((0, 250), (230, 250))], sb="dd")
    g["endash"] = Glyph([k.line((0, 250), (450, 250))], sb="dd")
    g["emdash"] = Glyph([k.line((0, 250), (820, 250))], sb="dd")
    g["slash"] = Glyph([k.line((0, -60), (330, cap + 40))], sb="dd")
    g["parenleft"] = Glyph([Stroke(k.arc(330, 265, 330, 520, 138, 222))], sb="dd")
    g["parenright"] = Glyph([Stroke(k.arc(0, 265, 330, 520, -42, 42))], sb="dd")
    g["quotesingle"] = Glyph([k.line((hv, cap), (hv, 540))], sb="ss")
    g["quotedbl"] = Glyph(
        [k.line((hv, cap), (hv, 540)), k.line((hv + 140, cap), (hv + 140, 540))], sb="ss"
    )
    g["plus"] = Glyph([k.line((0, 330), (380, 330)), k.line((190, 140), (190, 520))], sb="dd")
    g["equal"] = Glyph([k.line((0, 250), (380, 250)), k.line((0, 410), (380, 410))], sb="dd")
    g["underscore"] = Glyph([k.line((0, -60), (600, -60))], sb="dd")
    g["percent"] = Glyph(
        [
            k.ring(0, 220, 470, cap),
            k.ring(400, 620, 0, 230),
            k.line((70, -5), (550, cap + 5)),
        ],
        sb="rr",
    )
    g["space"] = Glyph([], [], sb="ss")
    return g
