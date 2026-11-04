"""Structural check of one Office or PDF file, run inside the sandbox.

Reads the file named on the command line from the working directory and prints
one JSON report on stdout. It never writes to that file, never opens a network
connection and never executes anything in the file. Its verdict is about the
container: a file that passes can still fail to open in Office, and a file that
fails is probably broken, which is why the caller reports the result as advisory.

With a second argument of `render` it also converts the file with LibreOffice
into `render/` (a PDF, up to three PNG pages, and for xlsx a recalculated copy
that is only read, never shipped). A third argument asks for that many PNG pages
of a deck, one per slide, at most 40 (`OFFICE_PREVIEW_PAGES`, EP-5 S9). Every
render failure is a warning: a missing or slow converter must not turn a clean
file into a failed one.

Exit status is 0 whenever a report was printed, including for a broken file. A
non-zero exit means the checker itself could not run, and the caller reports
that as skipped rather than as a fault in the file.
"""

import glob
import json
import os
import posixpath
import re
import shutil
import subprocess
import sys
import time
import xml.etree.ElementTree as ET
import zipfile

# Whole-package uncompressed ceiling. A real deliverable is far below this; a
# package above it is either a zip bomb or not a deliverable at all.
MAX_UNCOMPRESSED_BYTES = 256 * 1024 * 1024

MAIN_PART = {
    "docx": "word/document.xml",
    "xlsx": "xl/workbook.xml",
    "pptx": "ppt/presentation.xml",
}

# Names a sheet can be referred to by in a formula: quoted, or a bare word
# followed by '!'. Advisory, so a false match in a string literal is harmless.
SHEET_REF = re.compile(r"'((?:[^']|'')+)'!|([A-Za-z_][A-Za-z0-9_.]*)!")
ERROR_VALUES = {"#DIV/0!", "#REF!", "#NAME?", "#VALUE!", "#N/A", "#NUM!", "#NULL!"}


class Report:
    def __init__(self, kind):
        self.kind = kind
        self.checks = []
        self.stats = {}

    def fail(self, check_id, message):
        self.checks.append({"id": check_id, "level": "failed", "message": message})

    def warn(self, check_id, message):
        self.checks.append({"id": check_id, "level": "warnings", "message": message})

    def level(self):
        if any(check["level"] == "failed" for check in self.checks):
            return "failed"
        if any(check["level"] == "warnings" for check in self.checks):
            return "warnings"
        return "passed"

    def emit(self):
        print(
            json.dumps(
                {
                    "kind": self.kind,
                    "level": self.level(),
                    "checks": self.checks,
                    "stats": self.stats,
                }
            )
        )


def check_pdf(path, report):
    with open(path, "rb") as handle:
        data = handle.read()
    if not data.startswith(b"%PDF-"):
        report.fail("header", "the file does not start with a PDF header")
        return
    if b"%%EOF" not in data[-2048:]:
        report.fail("trailer", "the file has no end-of-file marker")
    if b"startxref" not in data:
        report.fail("xref", "the file has no cross-reference pointer")
    pages = len(re.findall(rb"/Type\s*/Page(?!s)", data))
    report.stats["page_objects"] = pages
    if pages == 0:
        report.warn("pages", "no page objects were visible; the pages may be compressed")


def unsafe_entry_name(name):
    if name.startswith("/") or "\\" in name or re.match(r"^[A-Za-z]:", name):
        return True
    return any(segment == ".." for segment in name.split("/"))


def local_name(tag):
    return tag.rsplit("}", 1)[-1]


def relationship_base(rels_name):
    # word/_rels/document.xml.rels belongs to word/document.xml, so targets
    # resolve against the directory above _rels.
    parts = rels_name.split("/")
    if len(parts) >= 2 and parts[-2] == "_rels":
        return "/".join(parts[:-2])
    return ""


def check_zip_package(path, kind, report):
    try:
        archive = zipfile.ZipFile(path)
    except zipfile.BadZipFile:
        report.fail("container", "the file is not a readable zip package (it may be truncated)")
        return
    except OSError:
        report.fail("container", "the file could not be opened as a zip package")
        return

    with archive:
        infos = archive.infolist()
        names = [info.filename for info in infos]
        report.stats["parts"] = len(names)
        if sum(info.file_size for info in infos) > MAX_UNCOMPRESSED_BYTES:
            report.fail("size", "the package decompresses to more than the check's limit")
            return

        unsafe = [name for name in names if unsafe_entry_name(name)]
        if unsafe:
            report.fail("names", "%d entr%s with an unsafe path" % (len(unsafe), "y" if len(unsafe) == 1 else "ies"))

        try:
            bad_member = archive.testzip()
        except (zipfile.BadZipFile, RuntimeError, OSError, ValueError):
            bad_member = "(unreadable)"
        if bad_member is not None:
            report.fail("crc", "an entry fails its checksum or cannot be read")

        if "[Content_Types].xml" not in names:
            report.fail("content_types", "[Content_Types].xml is missing")

        names_set = set(names)
        for name in names:
            if not (name.endswith(".xml") or name.endswith(".rels")):
                continue
            try:
                root = ET.fromstring(archive.read(name))
            except ET.ParseError:
                report.fail("xml", "malformed XML in one of the package parts")
                continue
            except (KeyError, RuntimeError, OSError, ValueError, zipfile.BadZipFile):
                report.fail("xml", "a package part cannot be read")
                continue
            if name.endswith(".rels"):
                check_relationships(root, name, names_set, report)

        main_part = MAIN_PART.get(kind)
        if main_part is not None and main_part not in names_set:
            report.fail("main_part", "the package has no %s part" % main_part)
            return

        if kind == "xlsx":
            check_workbook(archive, names, report)
        elif kind == "pptx":
            slides = len([name for name in names if re.match(r"^ppt/slides/slide\d+\.xml$", name)])
            report.stats["slides"] = slides
            if slides == 0:
                report.warn("slides", "the presentation has no slides")
        elif kind == "docx":
            paragraphs = 0
            try:
                root = ET.fromstring(archive.read(MAIN_PART["docx"]))
                paragraphs = sum(1 for node in root.iter() if local_name(node.tag) == "p")
            except (KeyError, ET.ParseError, RuntimeError, OSError, ValueError, zipfile.BadZipFile):
                pass
            report.stats["paragraphs"] = paragraphs


def check_relationships(root, rels_name, names_set, report):
    base = relationship_base(rels_name)
    for node in root.iter():
        if local_name(node.tag) != "Relationship":
            continue
        if node.get("TargetMode") == "External":
            continue
        target = node.get("Target")
        if not target:
            continue
        if target.startswith("/"):
            resolved = target.lstrip("/")
        else:
            resolved = posixpath.normpath(posixpath.join(base, target))
        if resolved not in names_set:
            report.fail("relationship", "a relationship points at a part that is not in the package")
            return


def check_workbook(archive, names, report):
    try:
        workbook = ET.fromstring(archive.read("xl/workbook.xml"))
    except (ET.ParseError, KeyError, RuntimeError, OSError, ValueError, zipfile.BadZipFile):
        return
    sheet_names = set()
    for node in workbook.iter():
        if local_name(node.tag) == "sheet" and node.get("name") is not None:
            sheet_names.add(node.get("name"))
    report.stats["sheets"] = len(sheet_names)

    formulas = 0
    error_cells = []
    missing_sheet_refs = 0
    sheet_parts = sorted(name for name in names if re.match(r"^xl/worksheets/[^/]+\.xml$", name))
    for part in sheet_parts:
        try:
            root = ET.fromstring(archive.read(part))
        except (ET.ParseError, KeyError, RuntimeError, OSError, ValueError, zipfile.BadZipFile):
            continue
        for cell in root.iter():
            tag = local_name(cell.tag)
            if tag == "c" and cell.get("t") == "e":
                value = ""
                for child in cell:
                    if local_name(child.tag) == "v" and child.text:
                        value = child.text.strip()
                error_cells.append(value)
            elif tag == "f":
                formulas += 1
                for match in SHEET_REF.finditer(cell.text or ""):
                    quoted, bare = match.group(1), match.group(2)
                    referred = quoted.replace("''", "'") if quoted is not None else bare
                    if referred not in sheet_names:
                        missing_sheet_refs += 1
                        break
    report.stats["formulas"] = formulas
    report.stats["error_cells"] = len(error_cells)
    if error_cells:
        known = sorted({value for value in error_cells if value in ERROR_VALUES})
        listed = ", ".join(known[:3]) if known else "an error value"
        report.fail("error_cells", "%d cell%s hold %s" % (len(error_cells), "" if len(error_cells) == 1 else "s", listed))
    if missing_sheet_refs:
        report.warn("formula_sheet", "%d formula%s refer%s to a sheet that does not exist" % (
            missing_sheet_refs, "" if missing_sheet_refs == 1 else "s", "s" if missing_sheet_refs == 1 else ""))


# Render layer. The conversions share one budget, kept under the caller's own
# timeout so a slow LibreOffice still lets the structural report come back.
RENDER_BUDGET_SECS = 50
POPPLER_TIMEOUT_SECS = 20
RENDER_DIR = "render"
PREVIEW_PDF = os.path.join(RENDER_DIR, "preview.pdf")
# The pages a preview draws, unless the caller asks for more of a deck, up to
# PREVIEW_PAGES_MAX (the deck slide cap, EP-5 D11).
PREVIEW_PAGES = 3
PREVIEW_PAGES_MAX = 40
PREVIEW_DPI = 60
# A deck with more thumbnails than PREVIEW_PAGES is drawn smaller, so the set
# stays under the verifier's byte budget. The default keeps the old size.
DECK_PREVIEW_DPI = 48
# Built into the image by the Dockerfile. Copied per check, so a run never
# changes the profile the next run starts from.
PREWARMED_PROFILE = "/opt/verevon/lo-profile"
# 0 makes LibreOffice recalculate formulas on load, so the recalculated copy
# holds results and not the values the file was saved with.
RECALC_ALWAYS = 0
FONT_ROW = re.compile(r"\s(yes|no)\s+(?:yes|no)\s+(?:yes|no)\s+\d+\s+\d+\s*$")
SUBSET_PREFIX = re.compile(r"^[A-Z]{6}\+")
# LibreOffice renders these Office families with an embedded metric-compatible
# stand-in, so an unembedded-font check never sees the substitution. Keys are
# font_key() forms.
METRIC_COMPATIBLE = {
    "calibri": "carlito",
    "cambria": "caladea",
    "arial": "liberationsans",
    "timesnewroman": "liberationserif",
    "couriernew": "liberationmono",
}
# Parts that name the fonts a file asks for, per kind. Theme references in them
# resolve against the theme part, so "minorHAnsi" is reported as the body font.
FONT_PARTS = {
    "docx": (r"^word/document\.xml$", r"^word/styles\.xml$"),
    "xlsx": (r"^xl/styles\.xml$",),
    "pptx": (r"^ppt/slides/slide\d+\.xml$",),
}
THEME_PART = {"docx": "word/theme/theme1.xml", "pptx": "ppt/theme/theme1.xml"}


def prepare_profile(profile):
    if os.path.isdir(PREWARMED_PROFILE):
        shutil.copytree(PREWARMED_PROFILE, profile)
    user = os.path.join(profile, "user")
    os.makedirs(user, exist_ok=True)
    # Macros and link updates stay off: a file must not run code or fetch
    # anything while it is being rendered.
    items = [
        ("/org.openoffice.Office.Common/Security/Scripting", "MacroSecurityLevel", 3),
        ("/org.openoffice.Office.Writer/Content/Update", "Link", 0),
        ("/org.openoffice.Office.Calc/Content/Update", "Link", 0),
        ("/org.openoffice.Office.Impress/Content/Update", "Link", 0),
        ("/org.openoffice.Office.Calc/Formula/Load", "OOXMLRecalcMode", RECALC_ALWAYS),
    ]
    body = "".join(
        '<item oor:path="%s"><prop oor:name="%s" oor:op="fuse"><value>%d</value></prop></item>\n'
        % (path, name, value)
        for path, name, value in items
    )
    header = (
        '<?xml version="1.0" encoding="UTF-8"?>\n'
        '<oor:items xmlns:oor="http://openoffice.org/2001/registry" '
        'xmlns:xs="http://www.w3.org/2001/XMLSchema" '
        'xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance">\n'
    )
    with open(os.path.join(user, "registrymodifications.xcu"), "w", encoding="utf-8") as handle:
        handle.write(header + body + "</oor:items>\n")


def soffice_convert(soffice, profile, source, out_dir, convert_to, produced_ext, deadline, report):
    remaining = deadline - time.monotonic()
    if remaining <= 0:
        report.warn("render", "the render time limit was reached before LibreOffice ran")
        return None
    os.makedirs(out_dir, exist_ok=True)
    command = [
        soffice,
        "--headless",
        "--norestore",
        "-env:UserInstallation=file://" + profile,
        "--convert-to",
        convert_to,
        "--outdir",
        out_dir,
        source,
    ]
    try:
        subprocess.run(
            command,
            stdin=subprocess.DEVNULL,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
            timeout=remaining,
            check=False,
        )
    except subprocess.TimeoutExpired:
        report.warn("render", "LibreOffice did not finish within the render time limit")
        return None
    except OSError:
        report.warn("render", "LibreOffice could not be started")
        return None
    stem = os.path.splitext(os.path.basename(source))[0]
    produced = os.path.join(out_dir, stem + "." + produced_ext)
    if not os.path.isfile(produced):
        report.warn("render", "LibreOffice did not produce a file from this document")
        return None
    return produced


def poppler(args, report):
    try:
        result = subprocess.run(
            args,
            stdin=subprocess.DEVNULL,
            stdout=subprocess.PIPE,
            stderr=subprocess.DEVNULL,
            timeout=POPPLER_TIMEOUT_SECS,
            check=False,
        )
    except (OSError, subprocess.TimeoutExpired):
        report.warn("render", "%s could not read the render (it may not be installed)" % args[0])
        return None
    if result.returncode != 0:
        report.warn("render", "%s could not read the render" % args[0])
        return None
    return result.stdout.decode("utf-8", "replace")


def font_key(family):
    return re.sub(r"[^a-z0-9]", "", family.lower())


def theme_fonts(archive, part):
    found = {"major": None, "minor": None}
    if part is None:
        return found
    try:
        root = ET.fromstring(archive.read(part))
    except (ET.ParseError, KeyError, RuntimeError, OSError, ValueError, zipfile.BadZipFile):
        return found
    for node in root.iter():
        if local_name(node.tag) in ("majorFont", "minorFont"):
            for child in node:
                if local_name(child.tag) == "latin" and child.get("typeface"):
                    found["major" if local_name(node.tag) == "majorFont" else "minor"] = child.get("typeface")
    return found


def named_fonts(node, kind, theme):
    tag = local_name(node.tag)
    if kind == "docx" and tag == "rFonts":
        for key, value in node.attrib.items():
            attribute = local_name(key)
            if attribute in ("ascii", "hAnsi"):
                yield value
            elif attribute in ("asciiTheme", "hAnsiTheme"):
                yield theme["major" if value.startswith("major") else "minor"]
    elif kind == "pptx" and tag == "latin":
        typeface = node.get("typeface") or ""
        if typeface.startswith("+mj"):
            yield theme["major"]
        elif typeface.startswith("+mn"):
            yield theme["minor"]
        else:
            yield typeface
    elif kind == "xlsx" and tag == "name":
        yield node.get("val")


def declared_fonts(path, kind):
    patterns = FONT_PARTS.get(kind)
    if patterns is None:
        return []
    try:
        archive = zipfile.ZipFile(path)
    except (zipfile.BadZipFile, OSError):
        return []
    families = {}
    with archive:
        theme = theme_fonts(archive, THEME_PART.get(kind))
        for name in sorted(names for names in archive.namelist() if any(re.match(p, names) for p in patterns)):
            try:
                root = ET.fromstring(archive.read(name))
            except (ET.ParseError, KeyError, RuntimeError, OSError, ValueError, zipfile.BadZipFile):
                continue
            for node in root.iter():
                for family in named_fonts(node, kind, theme):
                    if family and not family.startswith("+") and font_key(family):
                        families.setdefault(font_key(family), family)
    return list(families.values())


def font_rendered(family, rendered):
    wanted = {font_key(family)}
    substitute = METRIC_COMPATIBLE.get(font_key(family))
    if substitute is not None:
        wanted.add(substitute)
    return any(name.startswith(want) for name in rendered for want in wanted)


def page_number(path):
    # pdftoppm pads a page number to a width of its own choosing, so the names
    # sort as text only while every page has the same number of digits.
    match = re.search(r"(\d+)\.png$", path)
    return int(match.group(1)) if match else 0


def render_pdf_facts(pdf, declared, preview_pages, dpi, report):
    info = poppler(["pdfinfo", pdf], report)
    pages = None
    if info is not None:
        match = re.search(r"^Pages:\s+(\d+)", info, re.MULTILINE)
        pages = int(match.group(1)) if match else 0
        report.stats["render_pages"] = pages
        if pages == 0:
            report.warn("render_pages", "LibreOffice rendered no pages from this file")

    fonts = poppler(["pdffonts", pdf], report)
    if fonts is not None:
        rows = []
        for line in fonts.splitlines():
            match = FONT_ROW.search(line)
            if match:
                rows.append((match, line))
        not_embedded = sum(1 for match, _ in rows if match.group(1) == "no")
        report.stats["render_fonts"] = len(rows)
        report.stats["fonts_not_embedded"] = not_embedded
        if not_embedded:
            report.warn(
                "fonts",
                "%d font%s in the render %s not embedded, so Office may lay the text out differently"
                % (not_embedded, "" if not_embedded == 1 else "s", "is" if not_embedded == 1 else "are"),
            )
        # A font the file names but the render does not show was substituted by an
        # installed font, which is usually embedded, so the check above cannot see it.
        if declared and rows:
            rendered = {font_key(SUBSET_PREFIX.sub("", line.split()[0])) for _, line in rows}
            missing = [family for family in declared if not font_rendered(family, rendered)]
            report.stats["fonts_substituted"] = len(missing)
            if missing:
                report.warn(
                    "fonts",
                    "the file names %s, which the render does not show, so Office may lay the text out differently"
                    % ", ".join(missing[:3]),
                )

    if poppler(["pdftoppm", "-r", str(dpi), "-png", "-f", "1", "-l", str(preview_pages), pdf,
                os.path.join(RENDER_DIR, "page")], report) is not None:
        pages_png = sorted(glob.glob(os.path.join(RENDER_DIR, "page-*.png")), key=page_number)
        for index, path in enumerate(pages_png, 1):
            os.replace(path, os.path.join(RENDER_DIR, "thumb-%d.png" % index))

    text = poppler(["pdftotext", "-layout", "-f", "1", "-l", str(preview_pages), pdf, "-"], report)
    if text is not None:
        # pdftotext ends every page with a form feed, so the last piece is not a page.
        page_texts = text.split("\f")[:-1]
        blank = [str(index) for index, page in enumerate(page_texts[:preview_pages], 1) if not page.strip()]
        if blank:
            report.warn("blank_pages", "page %s has no text; check that it is not blank" % ", ".join(blank))


def recalc_errors(original, recalculated, report):
    try:
        import openpyxl
    except ImportError:
        report.warn("recalc", "openpyxl is not installed, so formula results were not compared")
        return
    try:
        formulas = openpyxl.load_workbook(original, data_only=False)
        values = openpyxl.load_workbook(recalculated, data_only=True)
    except Exception:  # noqa: BLE001 - an unreadable copy is a skipped check, not a finding
        report.warn("recalc", "the recalculated copy could not be read")
        return
    found = []
    for sheet in formulas.worksheets:
        if sheet.title not in values.sheetnames:
            continue
        shown_sheet = values[sheet.title]
        for row in sheet.iter_rows():
            for cell in row:
                if isinstance(cell.value, str) and cell.value.startswith("="):
                    shown = shown_sheet[cell.coordinate].value
                    if isinstance(shown, str) and shown in ERROR_VALUES:
                        found.append("%s!%s %s" % (sheet.title, cell.coordinate, shown))
    report.stats["recalc_errors"] = len(found)
    if found:
        report.fail(
            "recalc",
            "%d formula cell%s recalculate to an error, for example %s"
            % (len(found), "" if len(found) == 1 else "s", found[0]),
        )


def render_check(subject, kind, pages, report):
    soffice = shutil.which("soffice")
    if soffice is None:
        report.warn("render", "LibreOffice is not installed in this image, so the render checks were skipped")
        return
    deadline = time.monotonic() + RENDER_BUDGET_SECS
    profile = os.path.abspath("lo-profile")
    os.makedirs(RENDER_DIR, exist_ok=True)
    if kind == "pdf":
        shutil.copyfile(subject, PREVIEW_PDF)
    else:
        prepare_profile(profile)
        produced = soffice_convert(soffice, profile, subject, RENDER_DIR, "pdf", "pdf", deadline, report)
        if produced is None:
            return
        os.replace(produced, PREVIEW_PDF)
    dpi = DECK_PREVIEW_DPI if kind == "pptx" and pages > PREVIEW_PAGES else PREVIEW_DPI
    render_pdf_facts(PREVIEW_PDF, declared_fonts(subject, kind), pages, dpi, report)
    if kind == "xlsx":
        recalculated = soffice_convert(
            soffice, profile, subject, os.path.join(RENDER_DIR, "recalc"),
            "xlsx:Calc MS Excel 2007 XML", "xlsx", deadline, report,
        )
        if recalculated is not None:
            recalc_errors(subject, recalculated, report)


def preview_pages_arg(raw):
    # The caller clamps OFFICE_PREVIEW_PAGES, so a value it could not have sent is
    # refused here, which the caller reports as a skipped check, not rendered.
    if not (raw.isascii() and raw.isdigit()) or not 1 <= int(raw) <= PREVIEW_PAGES_MAX:
        sys.exit(2)
    return int(raw)


def main():
    # The arguments are the file, then `render`, then for a deck the pages to draw.
    if len(sys.argv) not in (2, 3, 4) or (len(sys.argv) > 2 and sys.argv[2] != "render"):
        sys.exit(2)
    pages = preview_pages_arg(sys.argv[3]) if len(sys.argv) == 4 else PREVIEW_PAGES
    subject = sys.argv[1]
    extension = subject.rsplit(".", 1)[-1].lower() if "." in subject else ""
    if extension not in ("docx", "xlsx", "pptx", "pdf"):
        sys.exit(2)
    report = Report(extension)
    try:
        if extension == "pdf":
            check_pdf(subject, report)
        else:
            check_zip_package(subject, extension, report)
    except Exception as error:  # noqa: BLE001 - any parse failure is a finding about the file
        report.fail("structure", "the file structure could not be read (%s)" % type(error).__name__)
    # A file that already failed the structure check is not worth the render time.
    if len(sys.argv) > 2 and report.level() != "failed":
        try:
            render_check(subject, extension, pages, report)
        except Exception as error:  # noqa: BLE001 - a render fault is advisory, never a file fault
            report.warn("render", "the render checks could not run (%s)" % type(error).__name__)
    report.emit()


if __name__ == "__main__":
    main()
