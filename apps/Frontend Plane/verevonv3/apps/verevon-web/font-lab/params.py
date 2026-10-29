"""Design parameters for the parametric Verevon Sans skeleton font.

Every glyph is a centre-line skeleton stroked with an axis-aligned elliptical
pen: `stem_v` is the thickness of vertical stems, `stem_h` of horizontal
strokes. Changing a weight only changes these numbers, never the skeleton.
"""

from dataclasses import dataclass, replace


@dataclass(frozen=True)
class Params:
    upm: int = 1000
    x_height: int = 530
    cap_height: int = 700
    ascender: int = 740
    descender: int = -210
    overshoot: int = 10

    # Pen thickness per weight matches Manrope's capital V (imported as-is), measured at cap height 700,
    # so every letter in the wordmark reads as the same stroke weight.
    stem_v: float = 57.0
    stem_h: float = 54.5

    # Superellipse exponent for bowls: 2 = true ellipse, higher = squarer.
    squareness: float = 2.35
    # Capital rounds (O Q C G Ø) follow a rounder, near-elliptical shape.
    squareness_caps: float = 2.0
    overshoot_caps: int = 14

    # Side bearings by edge type: s = straight stem, r = round, d = diagonal/open.
    sb_s: int = 74
    sb_r: int = 48
    sb_d: int = 22

    tabular_digit_advance: int = 560

    # Final shape: every outline is scaled by these after it is drawn. Compact (0.97) then 4% longer
    # and 4% shorter: width 0.97 * 1.04, height 0.96. The pen is compensated in Kit so the final
    # stroke thickness stays exactly stem_v / stem_h.
    longer: float = 1.04  # the "4% longer" step; imported outlines (Manrope V) get this but not the 0.97
    width_scale: float = 0.97 * 1.04
    height_scale: float = 0.96

    weight_class: int = 300
    style_name: str = "Light"


WEIGHTS = {
    "light": Params(),
    "regular": replace(
        Params(), stem_v=72.0, stem_h=68.5, sb_s=78, sb_r=52, sb_d=24,
        weight_class=400, style_name="Regular",
    ),
    "bold": replace(
        Params(), stem_v=118.0, stem_h=110.5, sb_s=84, sb_r=58, sb_d=28,
        weight_class=700, style_name="Bold",
    ),
}
