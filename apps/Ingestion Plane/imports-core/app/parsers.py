import csv
import hashlib
import io
import json
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
