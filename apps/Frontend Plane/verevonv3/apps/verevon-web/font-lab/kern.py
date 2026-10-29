"""Optical kerning computed from the glyph outlines.

For every pair the ink-to-ink gap is sampled on horizontal bands. Bands where
only one glyph has ink are ignored (nothing to collide with). The mean gap is
compared to the average of each glyph next to itself and the
difference, damped, becomes the kern value. A collision guard keeps a minimum
gap, so tightened pairs like A V never touch.
"""

STEP = 10
MAX_DEPTH = 260  # clamp huge open gaps so one wide counter doesn't dominate
DAMPING = 0.5
MIN_BANDS = 6
MIN_KERN = 16
MAX_NEG = -140
MAX_POS = 0  # tighten only; loosening from outline sampling proved unreliable
MIN_GAP_FRACTION = 0.45  # of the reference gap

LETTER_PUNCT = ()  # punctuation is kerned by hand-written rules below, not by outline sampling

# Hand-set pairs: open capitals / r before a full stop or comma.
PUNCT_RULES = {
    **{(a, p): -80 for a in "TVWYFP" for p in ("period", "comma")},
    **{(a, p): -40 for a in "rvwyL" for p in ("period", "comma")},
    # L only has its foot as ink on the right, so band sampling cannot see the open space.
    **{("L", b): v for b, v in (("T", -72), ("V", -60), ("W", -52), ("Y", -72), ("quotesingle", -60))},
}


def _crossings(poly, y):
    xs = []
    n = len(poly)
    for i in range(n):
        (x0, y0), (x1, y1) = poly[i], poly[(i + 1) % n]
        if (y0 <= y < y1) or (y1 <= y < y0):
            xs.append(x0 + (y - y0) * (x1 - x0) / (y1 - y0))
    return xs


def profile(polys, ys):
    """(left, right) ink extent per band, or None where the glyph has no ink."""
    out = []
    for y in ys:
        xs = [x for poly in polys for x in _crossings(poly, y)]
        out.append((min(xs), max(xs)) if xs else None)
    return out


def _gaps(left, right, adv_left):
    gaps = []
    for l, r in zip(left, right):
        if l is not None and r is not None:
            gaps.append(adv_left + r[0] - l[1])
    return gaps


def _mean_clamped(gaps):
    return sum(min(g, MAX_DEPTH + 120) for g in gaps) / len(gaps)


def compute(data, cap_height, x_height):
    """data: key -> (polygons, advance). Returns {(left_key, right_key): value}."""
    keys = [k for k in data if (k.isalpha() and len(k) == 1) or k in LETTER_PUNCT]
    caps_like = {k for k in keys if (len(k) == 1 and k.isupper()) or k in LETTER_PUNCT}

    prof = {}
    ys_cap = [y + STEP / 2 for y in range(0, cap_height, STEP)]
    ys_x = [y + STEP / 2 for y in range(0, x_height, STEP)]
    for k in keys:
        prof[k] = (profile(data[k][0], ys_x), profile(data[k][0], ys_cap))

    def band(k_l, k_r):
        use_cap = k_l in caps_like or k_r in caps_like
        idx = 1 if use_cap else 0
        return prof[k_l][idx], prof[k_r][idx]

    def ref_gap(a, b):
        l, r = band(a, b)
        gaps = _gaps(l, r, data[a][1])
        return _mean_clamped(gaps) if gaps else None

    self_gap = {k: ref_gap(k, k) for k in keys}

    pairs = {}
    for a in keys:
        for b in keys:
            l, r = band(a, b)
            gaps = _gaps(l, r, data[a][1])
            if len(gaps) < MIN_BANDS:
                continue
            if self_gap[a] is None or self_gap[b] is None:
                continue
            # A pair is "balanced" when it looks like the average of each glyph next to itself.
            ref = (self_gap[a] + self_gap[b]) / 2
            value = (ref - _mean_clamped(gaps)) * DAMPING
            value = max(MAX_NEG, min(MAX_POS, value))
            # collision guard: never close the tightest band below a fraction of the reference gap
            min_allowed = MIN_GAP_FRACTION * ref
            if min(gaps) + value < min_allowed:
                value = min(0, min_allowed - min(gaps))
            value = int(round(value / 4.0) * 4)
            if abs(value) >= MIN_KERN:
                pairs[(a, b)] = value
    pairs.update(PUNCT_RULES)
    return pairs
