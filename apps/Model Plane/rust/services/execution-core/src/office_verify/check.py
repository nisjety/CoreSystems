"""Structural check of one Office or PDF file, run inside the sandbox.

Reads the file named on the command line from the working directory and prints
one JSON report on stdout. It never writes, never opens a network connection and
never executes anything in the file. Its verdict is about the container: a file
that passes can still fail to open in Office, and a file that fails is probably
broken, which is why the caller reports the result as advisory.

Exit status is 0 whenever a report was printed, including for a broken file. A
non-zero exit means the checker itself could not run, and the caller reports
that as skipped rather than as a fault in the file.
"""

import json
import posixpath
import re
import sys
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


def main():
    if len(sys.argv) != 2:
        sys.exit(2)
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
    report.emit()


if __name__ == "__main__":
    main()
