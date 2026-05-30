import csv
import hashlib
import io
import json
from pathlib import Path
from typing import Any

from bs4 import BeautifulSoup
from docx import Document
from pypdf import PdfReader

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
    return "\n".join(paragraph.text for paragraph in document.paragraphs).strip()


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


def parse_uploaded_file(filename: str, content: bytes, content_type: str | None) -> ImportDocument:
    extension = Path(filename).suffix.lower().lstrip(".")
    metadata: dict[str, Any] = {
        "filename": filename,
        "content_type": content_type,
        "size_bytes": len(content),
        "sha256": _sha256(content),
        "extension": extension,
    }

    if extension == "pdf":
        text = _parse_pdf(content)
    elif extension == "docx":
        text = _parse_docx(content)
    elif extension in {"txt", "md"}:
        text = _parse_plain(content)
    elif extension == "csv":
        text = _parse_csv(content)
    elif extension == "json":
        text, extra = _parse_json(content)
        metadata.update(extra)
    elif extension in {"html", "htm"}:
        text = _parse_html(content)
    else:
        raise UnsupportedFileTypeError(f"Unsupported file type: {extension}")

    title = Path(filename).stem
    return ImportDocument(source_name=filename, title=title, text=text, metadata=metadata)
