"""Tests for build_deck.py.

From the repository root:

    python3 -m unittest discover -s "apps/Model Plane/skills/office/pptx/scripts" -p "test_*.py"

The validation tests use only the standard library. The tests that write a .pptx are
skipped when python-pptx is not installed, so this file runs in any python3.
"""

import base64
import contextlib
import importlib.util
import io
import json
import os
import shutil
import sys
import tempfile
import unittest

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)

# Imported after the path insert above, so the tests run from any working directory.
import build_deck  # noqa: E402

# The manifests execution-core keeps, so the Rust and Python sides read the same decks.
FIXTURES = os.path.join(
    HERE, "..", "..", "..", "..", "rust", "services", "execution-core", "tests", "fixtures", "deck"
)
HAVE_PPTX = importlib.util.find_spec("pptx") is not None
# A 1x1 PNG: small enough to embed, and python-pptx can read its size.
PIXEL_PNG = base64.b64decode(
    "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNkYPhfDwAChwGA60e6kgAAAABJRU5E"
    "rkJggg=="
)
SLIDE_W = 12_192_000
SLIDE_H = 6_858_000


def fixture(name):
    return os.path.join(FIXTURES, name)


def finding_ids(findings):
    return [finding["id"] for finding in findings]


class ValidationTests(unittest.TestCase):
    """Manifest and slide rules. Nothing is built here, so python-pptx is not needed."""

    def setUp(self):
        self.base = tempfile.mkdtemp()

    def tearDown(self):
        shutil.rmtree(self.base, ignore_errors=True)

    def plans(self, slides):
        manifest = {"version": 1, "title": "Test", "locale": "en-GB", "slides": slides}
        return build_deck.check_manifest(manifest, self.base)

    def test_overflowing_bullets_mark_the_slide_warn(self):
        manifest = build_deck.load_manifest(fixture("overflow_bullets.json"))
        plans = build_deck.check_manifest(manifest, self.base)
        self.assertEqual(build_deck.slide_state(plans[0]["findings"]), "ok")
        self.assertEqual(build_deck.slide_state(plans[1]["findings"]), "warn")
        self.assertIn("bullets_over_cap", finding_ids(plans[1]["findings"]))

    def test_a_title_over_cap_is_error(self):
        plans = self.plans(
            [{"id": "s1", "layout": "title_bullets", "title": "x" * 121, "bullets": ["Short"]}]
        )
        self.assertEqual(build_deck.slide_state(plans[0]["findings"]), "error")
        self.assertIn("title_too_long", finding_ids(plans[0]["findings"]))

    def test_a_title_of_exactly_the_cap_is_not_an_error(self):
        plans = self.plans(
            [{"id": "s1", "layout": "title_bullets", "title": "x" * 120, "bullets": ["Short"]}]
        )
        self.assertEqual(build_deck.slide_state(plans[0]["findings"]), "ok")

    def test_an_absolute_image_path_is_refused(self):
        plans = self.plans(
            [{"id": "s1", "layout": "image", "title": "Photo", "image": "/etc/hostname"}]
        )
        self.assertEqual(build_deck.slide_state(plans[0]["findings"]), "error")
        self.assertIn("image_path_absolute", finding_ids(plans[0]["findings"]))
        self.assertEqual(plans[0]["images"], [])

    def test_a_dotdot_image_path_is_refused(self):
        plans = self.plans(
            [{"id": "s1", "layout": "image", "title": "Photo", "image": "../photo.png"}]
        )
        self.assertIn("image_path_escapes", finding_ids(plans[0]["findings"]))
        self.assertEqual(plans[0]["images"], [])

    def test_the_bad_image_path_fixture_marks_both_picture_slides_error(self):
        manifest = build_deck.load_manifest(fixture("bad_image_path.json"))
        plans = build_deck.check_manifest(manifest, self.base)
        self.assertEqual(
            [build_deck.slide_state(plan["findings"]) for plan in plans],
            ["ok", "error", "error"],
        )

    def test_an_unknown_layout_is_error_not_crash(self):
        plans = self.plans(
            [{"id": "s1", "layout": "quote", "title": "Quote", "bullets": ["A line"]}]
        )
        self.assertEqual(len(plans), 1)
        self.assertEqual(build_deck.slide_state(plans[0]["findings"]), "error")
        self.assertIn("unknown_layout", finding_ids(plans[0]["findings"]))

    def test_a_number_without_notes_is_source_missing(self):
        plans = self.plans(
            [
                {
                    "id": "s1",
                    "layout": "title_bullets",
                    "title": "Growth",
                    "bullets": ["Revenue grew 12 percent"],
                }
            ]
        )
        self.assertEqual(build_deck.slide_state(plans[0]["findings"]), "warn")
        self.assertIn("source_missing", finding_ids(plans[0]["findings"]))

    def test_an_empty_deck_is_refused(self):
        manifest = build_deck.load_manifest(fixture("empty_deck.json"))
        with self.assertRaises(build_deck.Refusal) as caught:
            build_deck.check_manifest(manifest, self.base)
        self.assertEqual(caught.exception.finding["id"], "empty_deck")

    def test_a_bad_version_is_refused(self):
        manifest = {"version": 2, "title": "Test", "locale": "en-GB", "slides": []}
        with self.assertRaises(build_deck.Refusal) as caught:
            build_deck.check_manifest(manifest, self.base)
        self.assertEqual(caught.exception.finding["id"], "unsupported_version")

    def test_report_is_one_json_line(self):
        out = io.StringIO()
        with contextlib.redirect_stdout(out):
            status = build_deck.main(
                [fixture("empty_deck.json"), os.path.join(self.base, "deck.pptx")]
            )
        lines = out.getvalue().splitlines()
        self.assertEqual(status, 2)
        self.assertEqual(len(lines), 1)
        self.assertEqual(json.loads(lines[0])["refused"]["id"], "empty_deck")


@unittest.skipUnless(HAVE_PPTX, "python-pptx is not installed")
class BuildTests(unittest.TestCase):
    """Decks that are written and read back. These need python-pptx."""

    def setUp(self):
        self.base = tempfile.mkdtemp()

    def tearDown(self):
        shutil.rmtree(self.base, ignore_errors=True)

    def test_a_good_manifest_builds_and_reads_back(self):
        out = os.path.join(self.base, "deck.pptx")
        status, report = build_deck.run(fixture("good_5_slides.json"), out, base_dir=self.base)
        self.assertEqual(status, 0)
        self.assertEqual(report["deck"]["slides"], 5)
        self.assertEqual([slide["state"] for slide in report["slides"]], ["ok"] * 5)

        from pptx import Presentation

        deck = Presentation(out)
        self.assertEqual(len(deck.slides), 5)
        self.assertEqual(deck.slides[1].shapes.title.text_frame.text, "Omsetningen økte")
        self.assertEqual(deck.slide_width, SLIDE_W)
        self.assertEqual(deck.slide_height, SLIDE_H)

    def test_a_picture_is_placed_inside_the_slide(self):
        with open(os.path.join(self.base, "pixel.png"), "wb") as handle:
            handle.write(PIXEL_PNG)
        manifest = {
            "version": 1,
            "title": "Picture",
            "locale": "en-GB",
            "slides": [
                {"id": "s1", "layout": "image", "title": "One picture", "image": "pixel.png"}
            ],
        }
        manifest_path = os.path.join(self.base, "manifest.json")
        with open(manifest_path, "w", encoding="utf-8") as handle:
            json.dump(manifest, handle)
        out = os.path.join(self.base, "deck.pptx")
        status, report = build_deck.run(manifest_path, out, base_dir=self.base)
        self.assertEqual(status, 0)
        self.assertEqual(report["slides"][0]["state"], "ok")

        from pptx import Presentation
        from pptx.enum.shapes import MSO_SHAPE_TYPE

        slide = Presentation(out).slides[0]
        pictures = [shape for shape in slide.shapes if shape.shape_type == MSO_SHAPE_TYPE.PICTURE]
        self.assertEqual(len(pictures), 1)
        picture = pictures[0]
        self.assertGreaterEqual(picture.left, 0)
        self.assertLessEqual(picture.left + picture.width, SLIDE_W)
        self.assertGreaterEqual(picture.top, 0)
        self.assertLessEqual(picture.top + picture.height, SLIDE_H)

    def test_report_of_a_built_deck_is_one_json_line(self):
        out = io.StringIO()
        with contextlib.redirect_stdout(out):
            status = build_deck.main(
                [fixture("good_5_slides.json"), os.path.join(self.base, "deck.pptx")]
            )
        lines = out.getvalue().splitlines()
        self.assertEqual(status, 0)
        self.assertEqual(len(lines), 1)
        self.assertEqual(json.loads(lines[0])["deck"]["slides"], 5)


if __name__ == "__main__":
    unittest.main()
