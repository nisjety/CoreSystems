#!/usr/bin/env python3
"""Build a .pptx deck from a slide manifest (manifest v1) and report each slide's fit.

    python3 build_deck.py manifest.json out.pptx

The model writes the manifest and reads the report; it never places a shape itself.
The script writes the deck and prints one JSON line: how many slides were built and a
state for each, `ok`, `warn` or `error`. A refused manifest writes no file, and the
line names the reason under `refused`. Exit status is 0 when a file was written and 2
when the deck was refused.

Validation runs before python-pptx is imported, so a refused manifest needs no
library. The fit checks are structural estimates from the template's geometry and the
font sizes the builder sets. They say nothing about how PowerPoint or LibreOffice
substitute fonts, and the report never claims the deck was verified in PowerPoint.

The caps are EP-5 decision D11 (docs/plans/wave3/EP-5-deck-pipeline.md). The
server-side manifest contract in mp-contracts must hold the same numbers.
"""

import json
import math
import os
import re
import sys
import traceback

MANIFEST_VERSION = 1
MAX_MANIFEST_CHARS = 120_000
MAX_SLIDES = 40
MAX_BULLETS = 6
MAX_BULLET_CHARS = 90
MAX_TITLE_CHARS = 120
MAX_NOTES_CHARS = 2_000
MAX_IMAGES = 4
MAX_ID_CHARS = 64
LOCALE_PATTERN = re.compile(r"^[A-Za-z]{2,3}(-[A-Za-z0-9]{1,8})*$")

# Manifest layout -> default template layout name and the content fields it places.
# A field a layout does not place is reported as a finding, never dropped silently.
# The `title` layout places its bullets as the subtitle.
LAYOUTS = {
    "title": ("Title Slide", ("title", "bullets", "notes")),
    "title_bullets": ("Title and Content", ("title", "bullets", "notes")),
    "title_only": ("Title Only", ("title", "notes")),
    "section": ("Section Header", ("title", "bullets", "notes")),
    "two_column": ("Two Content", ("title", "left", "right", "notes")),
    "image": ("Title Only", ("title", "image", "notes")),
}
FALLBACK_LAYOUT = "title_bullets"
SLIDE_FIELDS = ("id", "layout", "title", "bullets", "left", "right", "image", "notes")

# A finding in this set makes its slide `error`. Every other finding makes it `warn`:
# the slide is built and can be presented, but the model should look at it.
ERROR_FINDINGS = frozenset(
    [
        "unknown_layout",
        "title_too_long",
        "empty_slide",
        "too_many_images",
        "image_path_invalid",
        "image_path_absolute",
        "image_path_escapes",
        "image_path_symlink",
        "image_missing",
        "image_unreadable",
        "shape_outside_slide",
        "build_failed",
    ]
)

# Geometry in EMU. The default python-pptx template is 4:3 and widen() stretches it to
# 16:9. IMAGE_BOX is the widened body area of that template.
EMU_PER_INCH = 914_400
SLIDE_W = 12_192_000
SLIDE_H = 6_858_000
IMAGE_BOX = (609_600, 1_600_200, 10_972_800, 4_525_963)  # left, top, width, height
IMAGE_GAP = 182_880  # 0.2 in between pictures in one row

# Text estimate. The glyph width is deliberately wide, so a line count is more often
# too high than too low. The insets are the template's default text insets.
BODY_PT = 18  # the SKILL's minimum body size
CHAR_EM = 0.55
LINE_EM = 1.2
PARAGRAPH_GAP_EM = 0.2
TEXT_INSET_IN = 0.2  # left and right together
TEXT_INSET_V_IN = 0.1  # top and bottom together


class Refusal(Exception):
    """The whole deck is refused: no file is written, and the report names the reason."""

    def __init__(self, finding_id, message):
        Exception.__init__(self, message)
        self.finding = {"id": finding_id, "message": message}


def short(exc):
    """One line of an exception's text, for a finding."""
    text = " ".join(str(exc).split())
    return text[:200] or type(exc).__name__


def refused_report(finding):
    return {"deck": {"slides": 0, "file": None}, "slides": [], "refused": finding}


def slide_state(findings):
    """`error` when any finding is an error, `warn` when there is any other, else `ok`."""
    if any(finding["id"] in ERROR_FINDINGS for finding in findings):
        return "error"
    return "warn" if findings else "ok"


def load_manifest(path):
    """Read and parse the manifest, refusing a file that cannot be used."""
    try:
        with open(path, "rb") as handle:
            raw = handle.read(4 * MAX_MANIFEST_CHARS + 1)
    except OSError as exc:
        raise Refusal("manifest_unreadable", "the manifest could not be read: " + short(exc))
    # UTF-8 takes at most four bytes a character, so a longer file is over the cap.
    if len(raw) > 4 * MAX_MANIFEST_CHARS:
        raise Refusal(
            "manifest_too_large", "the manifest is over %d characters" % MAX_MANIFEST_CHARS
        )
    try:
        text = raw.decode("utf-8-sig")
    except UnicodeDecodeError:
        raise Refusal("manifest_invalid", "the manifest is not UTF-8 text")
    if len(text) > MAX_MANIFEST_CHARS:
        raise Refusal(
            "manifest_too_large",
            "the manifest is %d characters; the limit is %d" % (len(text), MAX_MANIFEST_CHARS),
        )
    try:
        manifest = json.loads(text)
    except ValueError as exc:
        raise Refusal("manifest_invalid", "the manifest is not valid JSON: " + short(exc))
    if not isinstance(manifest, dict):
        raise Refusal("manifest_invalid", "the manifest must be a JSON object")
    return manifest


def check_manifest(manifest, base_dir):
    """Check the deck-level rules, then every slide. Returns one plan per slide.

    Raises Refusal when the deck cannot be built at all. A slide that breaks a rule is
    kept and marked, so the model can fix that slide and run the builder again.
    """
    version = manifest.get("version")
    if type(version) is not int or version != MANIFEST_VERSION:
        raise Refusal("unsupported_version", "the manifest version must be %d" % MANIFEST_VERSION)
    title = manifest.get("title")
    if not isinstance(title, str) or not title.strip():
        raise Refusal("manifest_invalid", "the manifest needs a non-empty title")
    locale = manifest.get("locale")
    if not isinstance(locale, str) or not LOCALE_PATTERN.match(locale):
        raise Refusal("manifest_invalid", "locale must be a language tag such as nb-NO")
    # Brand themes arrive in EP-5 S6. Until then a request for another theme is refused,
    # because a quietly ignored theme would look applied when it is not.
    if manifest.get("theme") not in (None, "default"):
        raise Refusal(
            "theme_unsupported", "only the default theme exists in this version; leave it out"
        )
    slides = manifest.get("slides")
    if not isinstance(slides, list):
        raise Refusal("manifest_invalid", "slides must be a list")
    if not slides:
        raise Refusal("empty_deck", "the deck has no slides")
    if len(slides) > MAX_SLIDES:
        raise Refusal(
            "too_many_slides",
            "a deck holds at most %d slides; this one has %d" % (MAX_SLIDES, len(slides)),
        )
    plans = []
    seen = set()
    for index, raw in enumerate(slides, 1):
        if not isinstance(raw, dict):
            raise Refusal("manifest_invalid", "slide %d is not an object" % index)
        slide_id = raw.get("id")
        if not isinstance(slide_id, str) or not slide_id.strip() or len(slide_id) > MAX_ID_CHARS:
            raise Refusal(
                "manifest_invalid",
                "slide %d needs an id of 1 to %d characters" % (index, MAX_ID_CHARS),
            )
        if slide_id in seen:
            raise Refusal("manifest_invalid", "the slide id %r is used twice" % slide_id)
        seen.add(slide_id)
        plans.append(check_slide(raw, index, slide_id, base_dir))
    return plans


def check_slide(raw, index, slide_id, base_dir):
    """Check one slide's layout, text and pictures. Findings are kept on the plan."""
    findings = []

    def flag(finding_id, message):
        findings.append({"id": finding_id, "message": message})

    layout = raw.get("layout")
    if isinstance(layout, str) and layout in LAYOUTS:
        template = layout
    else:
        template = FALLBACK_LAYOUT
        flag(
            "unknown_layout",
            "layout %r is not one of %s; the slide is built as %s"
            % (layout, ", ".join(LAYOUTS), FALLBACK_LAYOUT),
        )
    title = _line(_text(raw, "title", index))
    notes = _text(raw, "notes", index)
    bullets = _lines(raw, "bullets", index)
    left = _lines(raw, "left", index)
    right = _lines(raw, "right", index)
    image_list = _images(raw, index)
    listed = bullets + left + right

    if len(title) > MAX_TITLE_CHARS:
        flag(
            "title_too_long",
            "the title is %d characters; the limit is %d" % (len(title), MAX_TITLE_CHARS),
        )
    if len(listed) > MAX_BULLETS:
        flag(
            "bullets_over_cap",
            "%d bullets; the limit is %d per slide" % (len(listed), MAX_BULLETS),
        )
    if any(len(item) > MAX_BULLET_CHARS for item in listed):
        flag("bullet_too_long", "a bullet is over %d characters; shorten it" % MAX_BULLET_CHARS)
    if len(notes) > MAX_NOTES_CHARS:
        flag(
            "notes_too_long",
            "the notes are %d characters; the limit is %d" % (len(notes), MAX_NOTES_CHARS),
        )
    if not notes and any(re.search(r"\d", item) for item in listed):
        flag("source_missing", "a number is quoted but the notes name no source")

    placed = LAYOUTS[template][1]
    given = (("bullets", bullets), ("left", left), ("right", right), ("image", image_list))
    unplaced = [name for name, items in given if items and name not in placed]
    if unplaced:
        flag(
            "content_not_placed",
            "layout %s does not place %s, so they are left out" % (template, ", ".join(unplaced)),
        )
    for key in raw:
        if key not in SLIDE_FIELDS:
            flag("unknown_field", "%r is not a manifest v1 field and is ignored" % key)

    images = []
    if template == "image":
        if not image_list:
            flag("no_image", "the image layout has no picture")
        elif len(image_list) > MAX_IMAGES:
            flag(
                "too_many_images",
                "%d pictures; the limit is %d per slide, so none is placed"
                % (len(image_list), MAX_IMAGES),
            )
        else:
            for rel in image_list:
                problem = _image_problem(rel, base_dir)
                if problem is None:
                    images.append(os.path.normpath(os.path.join(base_dir, rel)))
                else:
                    flag(*problem)

    if not title:
        if listed or image_list:
            flag("title_missing", "the slide has no title")
        else:
            flag("empty_slide", "the slide has no title, text or picture")

    return {
        "id": slide_id,
        "index": index,
        "template": template,
        "title": title,
        "bullets": bullets,
        "left": left,
        "right": right,
        "notes": notes,
        "images": images,
        "findings": findings,
    }


def _text(raw, key, index):
    value = raw.get(key)
    if value is None:
        return ""
    if not isinstance(value, str):
        raise Refusal("manifest_invalid", "slide %d: %s must be a string" % (index, key))
    # A Windows line ending would otherwise leave a carriage return inside the paragraph.
    return value.replace("\r\n", "\n").strip()


def _line(text):
    """A title or bullet is one line: a line break in it would become a second paragraph."""
    return " ".join(text.split())


def _lines(raw, key, index):
    value = raw.get(key)
    if value is None:
        return []
    if not isinstance(value, list) or not all(isinstance(item, str) for item in value):
        raise Refusal("manifest_invalid", "slide %d: %s must be a list of strings" % (index, key))
    return [line for line in (_line(item) for item in value) if line]


def _images(raw, index):
    value = raw.get("image")
    if value is None:
        return []
    if isinstance(value, str):
        return [value]
    if isinstance(value, list) and all(isinstance(item, str) for item in value):
        return list(value)
    raise Refusal("manifest_invalid", "slide %d: image must be a path or a list of paths" % index)


def _image_problem(rel, base_dir):
    """Why a picture path may not be placed, as (finding id, message); None when it may."""
    if not rel.strip():
        return ("image_path_invalid", "an image path is empty")
    if rel.startswith(("/", "\\")) or os.path.isabs(rel) or re.match(r"^[A-Za-z]:", rel):
        return (
            "image_path_absolute",
            "image paths are relative to the working directory: %s" % rel,
        )
    parts = [part for part in re.split(r"[\\/]+", rel) if part not in ("", ".")]
    if ".." in parts:
        return ("image_path_escapes", "image paths may not use '..': %s" % rel)
    # Every step is checked, not only the file: a symlinked folder would lead outside.
    current = base_dir
    for part in parts:
        current = os.path.join(current, part)
        if os.path.islink(current):
            return (
                "image_path_symlink",
                "image paths may not pass through a symbolic link: %s" % rel,
            )
    if not os.path.isfile(current):
        return ("image_missing", "no image file at %s" % rel)
    return None


def write_deck(manifest, plans, out_path):
    """Build every slide and save the file. A slide that fails keeps its place, marked error."""
    try:
        from pptx import Presentation
    except ImportError:
        raise Refusal(
            "pptx_missing", "python-pptx is not installed for this python3; no deck can be built"
        )
    prs = Presentation()
    widen(prs)
    locale = manifest["locale"]
    prs.core_properties.title = manifest["title"].strip()[:255]
    prs.core_properties.language = locale
    layouts = {layout.name: layout for layout in prs.slide_layouts}
    for plan in plans:
        try:
            add_slide_to(prs, layouts, plan, locale)
        except Exception as exc:  # one broken slide must not lose the rest of the deck
            plan["findings"].append(
                {"id": "build_failed", "message": "the slide did not build: " + short(exc)}
            )
    try:
        prs.save(out_path)
    except Exception as exc:
        raise Refusal(
            "save_failed", "the deck could not be saved to %s: %s" % (out_path, short(exc))
        )


def widen(prs):
    """Stretch the 4:3 default template to 16:9 by scaling each placeholder across.

    Setting the slide width alone would leave every placeholder in the left three
    quarters of the slide. Layouts go first: a layout placeholder without its own
    position reads the master's, so the master must still be unscaled when they are read.
    """
    scale = SLIDE_W / float(prs.slide_width)
    prs.slide_width = SLIDE_W
    prs.slide_height = SLIDE_H
    for layout in prs.slide_layouts:
        for shape in layout.shapes:
            scale_shape(shape, scale)
    for shape in prs.slide_master.shapes:
        scale_shape(shape, scale)


def scale_shape(shape, scale):
    left, top, width, height = shape.left, shape.top, shape.width, shape.height
    if None in (left, top, width, height):
        return
    # All four are written, not only the horizontal pair: a position with one coordinate
    # missing is not valid DrawingML.
    shape.left = int(left * scale)
    shape.top = top
    shape.width = int(width * scale)
    shape.height = height


def add_slide_to(prs, layouts, plan, locale):
    template = plan["template"]
    slide = prs.slides.add_slide(layouts[LAYOUTS[template][0]])
    if plan["title"]:
        fill(slide.shapes.title, [plan["title"]], title_pt(plan["title"], template), locale)
    slots = {shape.placeholder_format.idx: shape for shape in slide.placeholders}
    if template in ("title", "title_bullets", "section"):
        fill_body(slots.get(1), plan["bullets"], locale)
    elif template == "two_column":
        fill_body(slots.get(1), plan["left"], locale)
        fill_body(slots.get(2), plan["right"], locale)
    elif template == "image":
        place_images(slide, plan["images"], plan["findings"])
    if plan["notes"]:
        notes_frame = slide.notes_slide.notes_text_frame
        if notes_frame is not None:
            notes_frame.text = plan["notes"]
    # An empty placeholder would show "Click to add text" when the deck is edited.
    for shape in list(slide.placeholders):
        if shape.has_text_frame and not shape.text_frame.text.strip():
            shape._element.getparent().remove(shape._element)
    fit_check(slide, plan["findings"])


def title_pt(text, template):
    """The title size whose two-line estimate fits the title box: smaller for longer titles."""
    if len(text) <= 36:
        return 40 if template == "title" else 36
    return 28 if len(text) <= 100 else 24


def fill(placeholder, paragraphs, size_pt, locale):
    from pptx.util import Pt

    frame = placeholder.text_frame
    for index, text in enumerate(paragraphs):
        paragraph = frame.paragraphs[0] if index == 0 else frame.add_paragraph()
        run = paragraph.add_run()
        run.text = text
        run.font.size = Pt(size_pt)
        run._r.get_or_add_rPr().set("lang", locale)


def fill_body(placeholder, items, locale):
    if placeholder is not None and items:
        fill(placeholder, items, BODY_PT, locale)


def place_images(slide, paths, findings):
    """Lay the pictures side by side in the body area, each scaled to fit its cell."""
    if not paths:
        return
    left, top, box_w, box_h = IMAGE_BOX
    cell_w = (box_w - IMAGE_GAP * (len(paths) - 1)) // len(paths)
    for index, path in enumerate(paths):
        cell_left = left + index * (cell_w + IMAGE_GAP)
        try:
            picture = slide.shapes.add_picture(path, cell_left, top, width=cell_w)
        except Exception as exc:
            findings.append(
                {"id": "image_unreadable", "message": "a picture could not be read: " + short(exc)}
            )
            continue
        if picture.height > box_h:
            picture.width = int(picture.width * box_h / picture.height)
            picture.height = box_h
        picture.left = cell_left + (cell_w - picture.width) // 2
        picture.top = top + (box_h - picture.height) // 2


def fit_check(slide, findings):
    """Structural fit: every shape inside the slide, and each text box's estimated height."""
    for shape in slide.shapes:
        if None in (shape.left, shape.top, shape.width, shape.height):
            continue
        if (
            shape.left < 0
            or shape.top < 0
            or shape.left + shape.width > SLIDE_W
            or shape.top + shape.height > SLIDE_H
        ):
            findings.append(
                {"id": "shape_outside_slide", "message": "a shape reaches outside the slide"}
            )
            break
    for shape in slide.placeholders:
        if not shape.has_text_frame or not shape.text_frame.text.strip():
            continue
        paragraphs = [paragraph.text for paragraph in shape.text_frame.paragraphs]
        if not fits(paragraphs, run_pt(shape), shape.width, shape.height):
            findings.append(
                {
                    "id": "text_may_overflow",
                    "message": "text may not fit its box; shorten it or split the slide",
                }
            )
            break


def run_pt(shape):
    """The size the builder set on a placeholder's text."""
    for paragraph in shape.text_frame.paragraphs:
        for run in paragraph.runs:
            if run.font.size is not None:
                return run.font.size.pt
    return float(BODY_PT)


def fits(paragraphs, size_pt, width, height):
    """Estimate whether the paragraphs fit a box: wrapped lines from the average glyph width."""
    width_in = width / EMU_PER_INCH - TEXT_INSET_IN
    height_in = height / EMU_PER_INCH - TEXT_INSET_V_IN
    per_line = max(1, int(width_in * 72 / (size_pt * CHAR_EM)))
    line_in = size_pt * LINE_EM / 72
    lines = sum(max(1, math.ceil(len(text) / per_line)) for text in paragraphs)
    return lines * line_in + len(paragraphs) * PARAGRAPH_GAP_EM * line_in <= height_in


def report_of(out_path, plans):
    return {
        "deck": {"slides": len(plans), "file": out_path},
        "slides": [
            {
                "id": plan["id"],
                "index": plan["index"],
                "state": slide_state(plan["findings"]),
                "findings": plan["findings"],
            }
            for plan in plans
        ],
    }


def run(manifest_path, out_path, base_dir=None):
    """Build the deck. Returns (exit status, report): 0 when a file was written, 2 when refused."""
    base = os.path.abspath(base_dir or os.getcwd())
    try:
        manifest = load_manifest(manifest_path)
        plans = check_manifest(manifest, base)
        write_deck(manifest, plans, out_path)
    except Refusal as refusal:
        return 2, refused_report(refusal.finding)
    except Exception as exc:  # a bug in this file: keep the one-line contract, traceback to stderr
        traceback.print_exc(file=sys.stderr)
        return 2, refused_report({"id": "internal_error", "message": short(exc)})
    return 0, report_of(out_path, plans)


def main(argv):
    if len(argv) != 2:
        status, report = 2, refused_report(
            {"id": "usage", "message": "usage: build_deck.py manifest.json out.pptx"}
        )
    else:
        status, report = run(argv[0], argv[1])
    print(json.dumps(report, separators=(",", ":")))
    return status


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
