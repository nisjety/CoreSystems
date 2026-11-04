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

    def cited_plans(self, slides, sources=None, claims=None, locale="nb-NO"):
        manifest = {"version": 1, "title": "Rapport", "locale": locale, "slides": slides}
        if sources is not None:
            manifest["sources"] = sources
        if claims is not None:
            manifest["unsupported_claims"] = claims
        return build_deck.check_manifest(manifest, self.base)

    def test_citation_numbers_move_to_notes(self):
        sources = [
            {"number": 1, "title": "SSB: Konsumprisindeksen", "url": "https://www.ssb.no/kpi"},
            {"number": 2, "url": "https://example.test/rapport"},
        ]
        plans = self.cited_plans(
            [
                {
                    "id": "s1",
                    "layout": "title_bullets",
                    "title": "Priser",
                    "bullets": ["Prisene steg 5 % [1]", "Strøm ble dyrere [2][1]"],
                    "notes": "Tall fra 2026.",
                }
            ],
            sources,
        )
        plan = plans[0]
        self.assertEqual(plan["bullets"], ["Prisene steg 5 %", "Strøm ble dyrere"])
        self.assertEqual(
            plan["notes"],
            "Tall fra 2026.\nKilde 1: SSB: Konsumprisindeksen\nKilde 2: https://example.test/rapport",
        )
        # The numbers are sourced in the notes now, so nothing is missing.
        self.assertEqual(build_deck.slide_state(plan["findings"]), "ok")

    def test_an_unknown_citation_stays_and_is_flagged(self):
        plans = self.cited_plans(
            [{"id": "s1", "layout": "title_bullets", "title": "Priser", "bullets": ["Opp [7]"]}],
            [{"number": 1, "title": "SSB"}],
            locale="en-GB",
        )
        self.assertEqual(plans[0]["bullets"], ["Opp [7]"])
        self.assertIn("citation_unknown", finding_ids(plans[0]["findings"]))
        self.assertEqual(build_deck.slide_state(plans[0]["findings"]), "warn")

    def test_without_sources_markers_are_left_alone(self):
        plans = self.plans(
            [
                {
                    "id": "s1",
                    "layout": "title_bullets",
                    "title": "Priser",
                    "bullets": ["Opp [1]"],
                    "notes": "Kilde: SSB",
                }
            ]
        )
        self.assertEqual(plans[0]["bullets"], ["Opp [1]"])
        self.assertEqual(plans[0]["notes"], "Kilde: SSB")

    def test_unsupported_claim_bullet_is_warned(self):
        # The fixture ledger: one claim the research marked unsupported.
        claims = ["Matprisene vil falle med ti prosent i 2027"]
        slide = {"id": "s1", "layout": "title_bullets", "title": "Utsikter", "notes": "Prognose"}
        bare = self.cited_plans(
            [dict(slide, bullets=["Matprisene vil falle med ti prosent i 2027 [1]"])],
            [{"number": 1, "title": "Rapport"}],
            claims,
        )
        self.assertIn("unsupported_claim", finding_ids(bare[0]["findings"]))
        self.assertEqual(build_deck.slide_state(bare[0]["findings"]), "warn")
        marked = self.cited_plans(
            [dict(slide, bullets=["Matprisene vil falle med ti prosent (ikke verifisert)"])],
            [],
            claims,
        )
        self.assertNotIn("unsupported_claim", finding_ids(marked[0]["findings"]))
        unrelated = self.cited_plans([dict(slide, bullets=["Strøm ble dyrere"])], [], claims)
        self.assertNotIn("unsupported_claim", finding_ids(unrelated[0]["findings"]))

    def test_bad_sources_refuse_the_deck(self):
        slides = [{"id": "s1", "layout": "title_only", "title": "Priser"}]
        for sources, claims in (
            ([{"number": 0, "title": "Null"}], None),
            ([{"number": 1}, {"number": 1}], None),
            ([{"number": 1, "title": 7}], None),
            ("SSB", None),
            (None, [3]),
        ):
            with self.assertRaises(build_deck.Refusal) as caught:
                self.cited_plans(slides, sources, claims)
            self.assertEqual(caught.exception.finding["id"], "manifest_invalid")

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

    def themed_bundle(self, theme, master=None):
        """A template bundle laid out as the /skills mount gives it; returns the theme path."""
        templates = os.path.join(self.base, "bundle", "templates")
        os.makedirs(templates)
        with open(os.path.join(templates, "logo.png"), "wb") as handle:
            handle.write(PIXEL_PNG)
        if master is not None:
            master.save(os.path.join(templates, "master.pptx"))
        path = os.path.join(templates, "deck-theme.json")
        with open(path, "w", encoding="utf-8") as handle:
            json.dump(theme, handle)
        return path

    def test_theme_colours_and_fonts_are_applied(self):
        theme = dict(BRAND_THEME)
        theme.pop("master")
        out = os.path.join(self.base, "deck.pptx")
        status, report = build_deck.run(
            fixture("good_5_slides.json"), out, base_dir=self.base, theme_path=self.themed_bundle(theme)
        )
        self.assertEqual(status, 0)
        self.assertTrue(report["theme"]["applied"])
        self.assertFalse(report["theme"]["master"])
        self.assertEqual(finding_ids(report["theme"]["findings"]), ["font_substituted"])

        from pptx import Presentation
        from pptx.enum.shapes import MSO_SHAPE_TYPE

        deck = Presentation(out)
        title_run = deck.slides[1].shapes.title.text_frame.paragraphs[0].runs[0]
        self.assertEqual(title_run.font.name, "Carlito")
        self.assertEqual(str(title_run.font.color.rgb), "0A3D62")
        body_runs = [
            run
            for shape in deck.slides[1].placeholders
            if shape.placeholder_format.idx == 1
            for paragraph in shape.text_frame.paragraphs
            for run in paragraph.runs
        ]
        self.assertTrue(body_runs)
        self.assertTrue(all(run.font.name == "Brand Sans Pro" for run in body_runs))
        self.assertTrue(all(str(run.font.color.rgb) == "1E272E" for run in body_runs))
        # The title slide carries the logo; a content slide carries the footer, inside the slide.
        pictures = [s for s in deck.slides[0].shapes if s.shape_type == MSO_SHAPE_TYPE.PICTURE]
        self.assertEqual(len(pictures), 1)
        footers = [s for s in deck.slides[1].shapes if s.has_text_frame and s.text_frame.text == "Aquatiq"]
        self.assertEqual(len(footers), 1)
        self.assertLessEqual(footers[0].top + footers[0].height, SLIDE_H)
        self.assertNotIn("shape_outside_slide", [i for s in report["slides"] for i in finding_ids(s["findings"])])

    def test_missing_master_layout_falls_back_with_a_finding(self):
        from pptx import Presentation

        master = Presentation()
        build_deck.widen(master)
        two_content = next(
            layout for layout in master.slide_layouts if layout.name == "Two Content"
        )
        master.slide_layouts.remove(two_content)
        # A sample slide in the master is not part of the deck.
        master.slides.add_slide(master.slide_layouts[0])
        manifest = {
            "version": 1,
            "title": "Brand",
            "locale": "nb-NO",
            "slides": [
                {"id": "s1", "layout": "two_column", "title": "Før og etter", "left": ["A"], "right": ["B"]},
                {"id": "s2", "layout": "title_bullets", "title": "Plan", "bullets": ["Steg en"]},
            ],
        }
        manifest_path = os.path.join(self.base, "manifest.json")
        with open(manifest_path, "w", encoding="utf-8") as handle:
            json.dump(manifest, handle)
        out = os.path.join(self.base, "deck.pptx")
        status, report = build_deck.run(
            manifest_path, out, base_dir=self.base, theme_path=self.themed_bundle(BRAND_THEME, master)
        )
        self.assertEqual(status, 0)
        self.assertTrue(report["theme"]["master"])
        self.assertIn("layout_missing_in_master", finding_ids(report["slides"][0]["findings"]))
        self.assertEqual(report["slides"][0]["state"], "warn")
        self.assertNotIn("layout_missing_in_master", finding_ids(report["slides"][1]["findings"]))
        self.assertEqual(len(Presentation(out).slides), 2)


# A theme as the contract fixture writes it (rust/crates/mp-contracts/tests/fixtures).
BRAND_THEME = {
    "version": 1,
    "name": "Aquatiq brand",
    "colors": {
        "primary": "#0A3D62",
        "secondary": "#3c6382",
        "text": "#1E272E",
        "background": "#FFFFFF",
        "accent": ["#E58E26"],
    },
    "fonts": {"heading": "Carlito", "body": "Brand Sans Pro"},
    "logo": "templates/logo.png",
    "master": "templates/master.pptx",
    "footer_text": "Aquatiq",
}


class ThemeTests(unittest.TestCase):
    """Reading a brand theme. Nothing is built here, so python-pptx is not needed."""

    def setUp(self):
        self.base = tempfile.mkdtemp()
        self.templates = os.path.join(self.base, "templates")
        os.makedirs(self.templates)
        with open(os.path.join(self.templates, "logo.png"), "wb") as handle:
            handle.write(PIXEL_PNG)

    def tearDown(self):
        shutil.rmtree(self.base, ignore_errors=True)

    def load(self, theme):
        path = os.path.join(self.templates, "deck-theme.json")
        with open(path, "w", encoding="utf-8") as handle:
            json.dump(theme, handle)
        return build_deck.load_theme(path)

    def test_a_valid_theme_is_usable_and_resolves_its_files(self):
        state = self.load(BRAND_THEME)
        self.assertTrue(state["usable"])
        self.assertEqual(state["logo"], os.path.join(self.base, "templates", "logo.png"))
        # No master.pptx in this bundle: reported, and the deck uses the default template.
        self.assertIsNone(state["master"])
        self.assertIn("master_unavailable", finding_ids(state["findings"]))
        self.assertIn("font_substituted", finding_ids(state["findings"]))

    def test_a_bad_theme_is_unavailable_not_a_refusal(self):
        for change in (
            {"colors": dict(BRAND_THEME["colors"], primary="red")},
            {"version": 2},
            {"script": "run.sh"},
            {"logo": "../logo.png"},
            {"master": "templates/other.pptx"},
            {"footer_text": "x" * 81},
        ):
            state = self.load(dict(BRAND_THEME, **change))
            self.assertFalse(state["usable"], change)
            self.assertEqual(finding_ids(state["findings"]), ["theme_unavailable"], change)

    def test_a_missing_theme_file_is_unavailable(self):
        state = build_deck.load_theme(os.path.join(self.templates, "absent.json"))
        self.assertFalse(state["usable"])
        self.assertEqual(finding_ids(state["findings"]), ["theme_unavailable"])

    def test_a_bundle_file_never_passes_through_a_link_or_dotdot(self):
        self.assertIsNone(build_deck.bundle_file(self.base, "templates/../templates/logo.png"))
        self.assertIsNone(build_deck.bundle_file(self.base, "/templates/logo.png"))
        self.assertIsNotNone(build_deck.bundle_file(self.base, "templates/logo.png"))
        if hasattr(os, "symlink"):
            try:
                os.symlink(self.templates, os.path.join(self.base, "linked"))
            except OSError:
                return
            self.assertIsNone(build_deck.bundle_file(self.base, "linked/logo.png"))

    def test_theme_flag_is_read_and_a_plain_build_has_no_theme_entry(self):
        out = io.StringIO()
        with contextlib.redirect_stdout(out):
            status = build_deck.main(["--theme", "a.json", "only-one-arg"])
        self.assertEqual(status, 2)
        self.assertEqual(json.loads(out.getvalue())["refused"]["id"], "usage")
        self.assertNotIn("theme", build_deck.report_of("deck.pptx", []))


if __name__ == "__main__":
    unittest.main()
