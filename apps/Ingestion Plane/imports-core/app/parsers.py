import csv
import hashlib
import io
import json
import re
import zipfile
import xml.etree.ElementTree as ET
from pathlib import Path
from typing import Any

import httpx
from bs4 import BeautifulSoup
from docx import Document
from pypdf import PdfReader

from app.config import get_settings
from app.schemas import ImportDocument


class UnsupportedFileTypeError(ValueError):
    pass


def _sha256(content: bytes) -> str:
    return hashlib.sha256(content).hexdigest()


def _parse_pdf(content: bytes) -> str:
    reader = PdfReader(io.BytesIO(content))
    parts: list[str] = []
    for page in reader.pages:
        parts.append(page.extract_text() or "")
    return "\n".join(parts).strip()


def _parse_docx(content: bytes) -> str:
    document = Document(io.BytesIO(content))
    # Preserve paragraph/table order: order quantities and policy revisions
    # frequently live in tables, not in document.paragraphs.
    from docx.table import Table
    from docx.text.paragraph import Paragraph
    parts = []
    for element in document.element.body:
        if element.tag.endswith('}p'):
            parts.append(Paragraph(element, document).text)
        elif element.tag.endswith('}tbl'):
            table = Table(element, document)
            parts.extend('\t'.join(cell.text for cell in row.cells) for row in table.rows)
    return "\n".join(parts).strip()


def _parse_plain(content: bytes) -> str:
    return content.decode("utf-8", errors="ignore").strip()


def _parse_csv(content: bytes) -> str:
    decoded = content.decode("utf-8", errors="ignore")
    rows = list(csv.reader(io.StringIO(decoded)))
    return "\n".join("\t".join(row) for row in rows)


def _parse_json(content: bytes) -> tuple[str, dict[str, Any]]:
    payload = json.loads(content.decode("utf-8", errors="ignore"))
    if isinstance(payload, dict):
        keys = list(payload.keys())
    elif isinstance(payload, list):
        keys = ["<array>"]
    else:
        keys = [type(payload).__name__]
    return json.dumps(payload, ensure_ascii=False, indent=2), {"json_keys": keys}


def _parse_html(content: bytes) -> str:
    soup = BeautifulSoup(content, "lxml")
    return soup.get_text("\n", strip=True)


# ---------------------------------------------------------------------------
# XLSX / PPTX: both are zip containers of plain XML parts (OOXML), so the
# standard library's zipfile + xml.etree cover them without a new dependency
# (mirrors the Go extractor's archive/zip + encoding/xml approach).

_XLSX_MAIN_NS = "http://schemas.openxmlformats.org/spreadsheetml/2006/main"
_OOXML_REL_NS = "http://schemas.openxmlformats.org/officeDocument/2006/relationships"
_PACKAGE_REL_NS = "http://schemas.openxmlformats.org/package/2006/relationships"
_PPTX_MAIN_NS = "http://schemas.openxmlformats.org/presentationml/2006/main"
_DRAWINGML_NS = "http://schemas.openxmlformats.org/drawingml/2006/main"

_CELL_REF_RE = re.compile(r"^([A-Z]+)(\d+)$")


class _MalformedOfficeDocument(ValueError):
    """A .xlsx/.pptx zip is missing or has malformed OOXML parts."""


def _read_zip_xml(zf: zipfile.ZipFile, path: str) -> ET.Element | None:
    if path not in zf.namelist():
        return None
    try:
        return ET.fromstring(zf.read(path))
    except ET.ParseError as exc:
        raise _MalformedOfficeDocument(f"Malformed XML part: {path}") from exc


def _relationship_targets(zf: zipfile.ZipFile, rels_path: str) -> dict[str, str]:
    """Map relationship id -> target path, from a `_rels/*.rels` part."""
    root = _read_zip_xml(zf, rels_path)
    if root is None:
        return {}
    return {
        rel.get("Id", ""): rel.get("Target", "")
        for rel in root.findall(f"{{{_PACKAGE_REL_NS}}}Relationship")
    }


def _col_letters_to_index(letters: str) -> int:
    """Spreadsheet column letters ("A", "Z", "AA", ...) to a 1-based index."""
    index = 0
    for char in letters:
        index = index * 26 + (ord(char) - ord("A") + 1)
    return index


def _xlsx_shared_strings(zf: zipfile.ZipFile) -> list[str]:
    root = _read_zip_xml(zf, "xl/sharedStrings.xml")
    if root is None:
        return []
    ns = {"m": _XLSX_MAIN_NS}
    return [
        "".join(t.text or "" for t in si.findall(".//m:t", ns))
        for si in root.findall("m:si", ns)
    ]


def _xlsx_sheet_parts(zf: zipfile.ZipFile) -> list[tuple[str, str]]:
    """Return `[(sheet_name, worksheet_zip_path), ...]` in workbook order."""
    workbook = _read_zip_xml(zf, "xl/workbook.xml")
    if workbook is None:
        raise _MalformedOfficeDocument("xl/workbook.xml is missing")
    targets = _relationship_targets(zf, "xl/_rels/workbook.xml.rels")
    ns = {"m": _XLSX_MAIN_NS}
    parts: list[tuple[str, str]] = []
    for sheet in workbook.findall(".//m:sheets/m:sheet", ns):
        name = sheet.get("name", "")
        rid = sheet.get(f"{{{_OOXML_REL_NS}}}id", "")
        target = targets.get(rid, "")
        if not target:
            continue
        # Target is normally relative to `xl/` ("worksheets/sheet1.xml"), but
        # OPC also allows a package-root-absolute form ("/xl/worksheets/...").
        # Strip the leading slash *before* checking for the "xl/" prefix, or
        # an absolute target doubles up into "xl/xl/worksheets/..." and the
        # sheet silently reads as empty instead of being found.
        target = target.lstrip('/')
        path = target if target.startswith("xl/") else f"xl/{target}"
        parts.append((name, path))
    return parts


def _xlsx_cell_text(cell: ET.Element, ns: dict[str, str], shared: list[str]) -> str:
    cell_type = cell.get("t")
    if cell_type == "inlineStr":
        inline = cell.find("m:is", ns)
        return "".join(t.text or "" for t in inline.findall(".//m:t", ns)) if inline is not None else ""
    value = cell.find("m:v", ns)
    if value is None or value.text is None:
        return ""
    if cell_type == "s":
        try:
            return shared[int(value.text)]
        except (ValueError, IndexError):
            return ""
    return value.text


def _xlsx_sheet_rows(zf: zipfile.ZipFile, path: str, shared: list[str]) -> list[str]:
    root = _read_zip_xml(zf, path)
    if root is None:
        return []
    ns = {"m": _XLSX_MAIN_NS}
    rows: list[str] = []
    for row in root.findall(".//m:sheetData/m:row", ns):
        by_column: dict[int, str] = {}
        next_column = 1
        for cell in row.findall("m:c", ns):
            match = _CELL_REF_RE.match(cell.get("r", ""))
            column = _col_letters_to_index(match.group(1)) if match else next_column
            by_column[column] = _xlsx_cell_text(cell, ns, shared)
            next_column = column + 1
        last_column = max(by_column, default=0)
        rows.append("\t".join(by_column.get(i, "") for i in range(1, last_column + 1)))
    return rows


def _parse_xlsx(content: bytes) -> str:
    """Sheet names and cell values (via shared strings) as tab-separated rows
    per sheet, in workbook order."""
    try:
        with zipfile.ZipFile(io.BytesIO(content)) as zf:
            shared = _xlsx_shared_strings(zf)
            sections = []
            for name, path in _xlsx_sheet_parts(zf):
                rows = _xlsx_sheet_rows(zf, path, shared)
                sections.append("\n".join([f"[{name}]", *rows]))
            return "\n\n".join(sections).strip()
    except zipfile.BadZipFile as exc:
        raise _MalformedOfficeDocument("Not a valid .xlsx archive") from exc


def _pptx_slide_parts(zf: zipfile.ZipFile) -> list[str]:
    """Return slide XML zip paths in the presentation's own slide order."""
    presentation = _read_zip_xml(zf, "ppt/presentation.xml")
    if presentation is None:
        raise _MalformedOfficeDocument("ppt/presentation.xml is missing")
    targets = _relationship_targets(zf, "ppt/_rels/presentation.xml.rels")
    ns = {"p": _PPTX_MAIN_NS}
    paths: list[str] = []
    for sld_id in presentation.findall(".//p:sldIdLst/p:sldId", ns):
        rid = sld_id.get(f"{{{_OOXML_REL_NS}}}id", "")
        target = targets.get(rid, "")
        if not target:
            continue
        # See the matching comment in _xlsx_sheet_parts: strip before prefixing
        # so a package-root-absolute target doesn't double up to "ppt/ppt/...".
        target = target.lstrip('/')
        paths.append(target if target.startswith("ppt/") else f"ppt/{target}")
    return paths


def _pptx_slide_text(zf: zipfile.ZipFile, path: str) -> str:
    root = _read_zip_xml(zf, path)
    if root is None:
        return ""
    ns = {"a": _DRAWINGML_NS}
    paragraphs = [
        "".join(t.text or "" for t in paragraph.findall(".//a:t", ns))
        for paragraph in root.findall(".//a:p", ns)
    ]
    return "\n".join(text for text in paragraphs if text.strip())


def _parse_pptx(content: bytes) -> str:
    """Slide text in presentation order, one section per slide."""
    try:
        with zipfile.ZipFile(io.BytesIO(content)) as zf:
            sections = []
            for index, path in enumerate(_pptx_slide_parts(zf), start=1):
                text = _pptx_slide_text(zf, path)
                sections.append("\n".join([f"[Slide {index}]", *([text] if text else [])]))
            return "\n\n".join(sections).strip()
    except zipfile.BadZipFile as exc:
        raise _MalformedOfficeDocument("Not a valid .pptx archive") from exc


def _parse_tika(
    filename: str,
    content: bytes,
    content_type: str | None,
    tika_url: str,
    timeout_s: float,
) -> str:
    headers = {
        "Accept": "text/plain",
        "Content-Type": content_type or "application/octet-stream",
        "Content-Disposition": f'attachment; filename="{Path(filename).name}"',
    }
    with httpx.Client(timeout=timeout_s) as client:
        response = client.put(
            f"{tika_url.rstrip('/')}/tika",
            content=content,
            headers=headers,
        )
        response.raise_for_status()
        return response.text.strip()


def _parse_local(
    extension: str,
    content: bytes,
    metadata: dict[str, Any],
) -> str:
    if extension == "pdf":
        return _parse_pdf(content)
    if extension == "docx":
        return _parse_docx(content)
    if extension in {"txt", "md"}:
        return _parse_plain(content)
    if extension == "csv":
        return _parse_csv(content)
    if extension == "json":
        text, extra = _parse_json(content)
        metadata.update(extra)
        return text
    if extension in {"html", "htm"}:
        return _parse_html(content)
    if extension == "xlsx":
        return _parse_xlsx(content)
    if extension == "pptx":
        return _parse_pptx(content)
    raise UnsupportedFileTypeError(f"Unsupported file type: {extension}")


def parse_uploaded_file(
    filename: str,
    content: bytes,
    content_type: str | None,
) -> ImportDocument:
    extension = Path(filename).suffix.lower().lstrip(".")
    settings = get_settings()
    metadata: dict[str, Any] = {
        "filename": filename,
        "content_type": content_type,
        "size_bytes": len(content),
        "sha256": _sha256(content),
        "extension": extension,
    }

    text = ""
    last_tika_error: str | None = None
    processor_order = settings.document_processor_order or ["local"]
    for processor in processor_order:
        if processor == "tika":
            if not settings.tika_enabled:
                continue
            try:
                text = _parse_tika(
                    filename,
                    content,
                    content_type,
                    settings.tika_url,
                    settings.tika_timeout_s,
                )
                if text:
                    metadata["parser"] = "tika"
                    break
            except (httpx.HTTPError, OSError) as exc:
                last_tika_error = exc.__class__.__name__
                continue
        elif processor == "local":
            text = _parse_local(extension, content, metadata)
            metadata["parser"] = "local"
            break

    if not text and "local" not in processor_order:
        text = _parse_local(extension, content, metadata)
        metadata["parser"] = "local"

    if last_tika_error and metadata.get("parser") != "tika":
        metadata["tika_fallback_error"] = last_tika_error

    title = Path(filename).stem
    return ImportDocument(source_name=filename, title=title, text=text, metadata=metadata)
