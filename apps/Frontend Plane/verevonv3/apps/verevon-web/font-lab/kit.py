"""Geometry helpers shared by all glyph definitions."""

import math
from dataclasses import dataclass, field

from stroker import Stroke, _off, superellipse_fill


@dataclass
class Glyph:
    strokes: list = field(default_factory=list)
    fills: list = field(default_factory=list)
    sb: str = "ss"  # left/right edge type: s straight, r round, d diagonal
    exact: bool = False  # True: imported outline, skip the global width scale


class Kit:
    def __init__(self, p):
        self.p = p
        self.hv = p.stem_v / 2 / p.width_scale
        self.hh = p.stem_h / 2 / p.height_scale
        self.xh = p.x_height
        self.cap = p.cap_height
        self.asc = p.ascender
        self.desc = p.descender
        self.ov = p.overshoot

    # -- primitives ---------------------------------------------------------
    def arc(self, cx, cy, rx, ry, t0, t1, n=None, step=5.0):
        k = 2.0 / (n or self.p.squareness)
        cnt = max(2, int(math.ceil(abs(t1 - t0) / step)))
        pts = []
        for i in range(cnt + 1):
            t = math.radians(t0 + (t1 - t0) * i / cnt)
            c, s = math.cos(t), math.sin(t)
            pts.append((cx + rx * math.copysign(abs(c) ** k, c), cy + ry * math.copysign(abs(s) ** k, s)))
        return pts

    def ring(self, x0, x1, y0, y1, n=None):
        """Closed bowl whose ink box is (x0,y0)-(x1,y1)."""
        pts = self.arc((x0 + x1) / 2, (y0 + y1) / 2, (x1 - x0) / 2 - self.hv, (y1 - y0) / 2 - self.hh, 0, 360, n=n)
        return Stroke(pts[:-1], closed=True)

    def line(self, *pts, cap0=None, cap1=None):
        return Stroke(list(pts), cap0=cap0, cap1=cap1)

    def path(self, *parts, cap0=None, cap1=None):
        """Concatenate point lists / single points into one open stroke."""
        pts = []
        for part in parts:
            if isinstance(part, tuple):
                pts.append(part)
            else:
                pts.extend(part)
        return Stroke(pts, cap0=cap0, cap1=cap1)

    def dot(self, cx, cy, r=None):
        r = r or (self.hv + 9)
        return superellipse_fill(cx, cy, r, r)

    def hx(self, dx, dy):
        """Half of the horizontal ink width where a flat-cut stroke with direction (dx, dy) ends."""
        d = math.hypot(dx, dy)
        ux, uy = dx / d, dy / d
        ox, oy = _off(self.hv, self.hh, -uy, ux)
        return math.hypot(ox, oy) / max(abs(uy), 0.2)

    # -- derived metrics ----------------------------------------------------
    @property
    def top_c(self):  # centre-line y of a flat x-height edge
        return self.xh - self.hh

    @property
    def round_top_c(self):  # centre-line y of an overshooting round at x-height
        return self.xh + self.ov - self.hh

    @property
    def round_bot_c(self):
        return -self.ov + self.hh


def translate(strokes, dx, dy):
    out = []
    for s in strokes:
        out.append(Stroke([(x + dx, y + dy) for x, y in s.pts], s.closed, s.cap0, s.cap1))
    return out


def translate_fills(fills, dx, dy):
    return [[(x + dx, y + dy) for x, y in f] for f in fills]


def rot180(strokes, cx, cy):
    flip = {"h": "h", "v": "v", None: None}
    return [
        Stroke([(2 * cx - x, 2 * cy - y) for x, y in s.pts], s.closed, flip[s.cap0], flip[s.cap1])
        for s in strokes
    ]
