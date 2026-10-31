"""Turns centre-line strokes into filled outline polygons.

The pen is an axis-aligned ellipse (half-axes hv, hh), so thickness follows
the stroke direction automatically: vertical strokes get hv, horizontal ones
hh. Output polygons are clockwise (TrueType outer-contour orientation) and
are meant to be filled with the non-zero rule, so overlapping strokes need
no boolean union.
"""

import math
from dataclasses import dataclass

COS_CORNER = math.cos(math.radians(24))
MITER_LIMIT = 3.0


@dataclass
class Stroke:
    pts: list
    closed: bool = False
    cap0: str | None = None  # None = perpendicular butt, 'h' / 'v' = cut line direction
    cap1: str | None = None


def _unit(dx, dy):
    d = math.hypot(dx, dy)
    return (dx / d, dy / d) if d > 1e-9 else (0.0, 0.0)


def _off(hv, hh, nx, ny):
    d = math.hypot(hv * nx, hh * ny)
    if d < 1e-9:
        return (0.0, 0.0)
    return (hv * hv * nx / d, hh * hh * ny / d)


def _dedupe(pts, closed):
    out = []
    for p in pts:
        if not out or math.hypot(p[0] - out[-1][0], p[1] - out[-1][1]) > 1e-6:
            out.append(p)
    if closed and len(out) > 1 and math.hypot(out[0][0] - out[-1][0], out[0][1] - out[-1][1]) < 1e-6:
        out.pop()
    return out


def _cut(p, q, t, kind):
    if kind is None:
        return q
    cn = (0.0, 1.0) if kind == "h" else (1.0, 0.0)
    tc = t[0] * cn[0] + t[1] * cn[1]
    if abs(tc) < 0.25:
        return q
    s = -((q[0] - p[0]) * cn[0] + (q[1] - p[1]) * cn[1]) / tc
    return (q[0] + t[0] * s, q[1] + t[1] * s)


def _vertex(p, t0, t1, hv, hh):
    """Left and right offset point lists at a vertex between tangents t0 and t1."""
    n0 = (-t0[1], t0[0])
    n1 = (-t1[1], t1[0])
    dot = t0[0] * t1[0] + t0[1] * t1[1]
    cross = t0[0] * t1[1] - t0[1] * t1[0]
    result = []
    for s in (1, -1):
        if dot > COS_CORNER or abs(cross) < 1e-9:
            nm = _unit(n0[0] + n1[0], n0[1] + n1[1])
            o = _off(hv, hh, s * nm[0], s * nm[1])
            result.append([(p[0] + o[0], p[1] + o[1])])
            continue
        o0 = _off(hv, hh, s * n0[0], s * n0[1])
        o1 = _off(hv, hh, s * n1[0], s * n1[1])
        d = (o1[0] - o0[0], o1[1] - o0[1])
        a = (d[0] * t1[1] - d[1] * t1[0]) / cross
        tip = (p[0] + o0[0] + a * t0[0], p[1] + o0[1] + a * t0[1])
        if math.hypot(tip[0] - p[0], tip[1] - p[1]) > MITER_LIMIT * max(hv, hh):
            result.append([(p[0] + o0[0], p[1] + o0[1]), (p[0] + o1[0], p[1] + o1[1])])
        else:
            result.append([tip])
    return result[0], result[1]


def _area(poly):
    return 0.5 * sum(
        poly[i][0] * poly[(i + 1) % len(poly)][1] - poly[(i + 1) % len(poly)][0] * poly[i][1]
        for i in range(len(poly))
    )


def orient(poly, clockwise=True):
    return poly if (_area(poly) < 0) == clockwise else poly[::-1]


def stroke_to_polygons(stroke, hv, hh):
    pts = _dedupe(stroke.pts, stroke.closed)
    n = len(pts)
    if n < 2:
        return []
    m = n if stroke.closed else n - 1
    tan = [_unit(pts[(i + 1) % n][0] - pts[i][0], pts[(i + 1) % n][1] - pts[i][1]) for i in range(m)]

    left, right = [], []
    for i in range(n):
        p = pts[i]
        if stroke.closed:
            lp, rp = _vertex(p, tan[i - 1], tan[i], hv, hh)
        elif i == 0:
            nx, ny = -tan[0][1], tan[0][0]
            o = _off(hv, hh, nx, ny)
            lp = [_cut(p, (p[0] + o[0], p[1] + o[1]), tan[0], stroke.cap0)]
            rp = [_cut(p, (p[0] - o[0], p[1] - o[1]), tan[0], stroke.cap0)]
        elif i == n - 1:
            nx, ny = -tan[-1][1], tan[-1][0]
            o = _off(hv, hh, nx, ny)
            lp = [_cut(p, (p[0] + o[0], p[1] + o[1]), tan[-1], stroke.cap1)]
            rp = [_cut(p, (p[0] - o[0], p[1] - o[1]), tan[-1], stroke.cap1)]
        else:
            lp, rp = _vertex(p, tan[i - 1], tan[i], hv, hh)
        left.extend(lp)
        right.extend(rp)

    if stroke.closed:
        loops = [left, right]
        loops.sort(key=lambda q: -abs(_area(q)))
        return [orient(loops[0], True), orient(loops[1], False)]
    return [orient(left + right[::-1], True)]


def superellipse_fill(cx, cy, rx, ry, n=2.6, step=10):
    pts = []
    k = 2.0 / n
    for a in range(0, 360, step):
        t = math.radians(a)
        c, s = math.cos(t), math.sin(t)
        pts.append((cx + rx * math.copysign(abs(c) ** k, c), cy + ry * math.copysign(abs(s) ** k, s)))
    return orient(pts, True)
