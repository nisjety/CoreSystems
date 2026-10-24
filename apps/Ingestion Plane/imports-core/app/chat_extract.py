"""Bounded, local-only document extraction. No jobs, database or external parser."""
import base64
import binascii
import os
from pathlib import Path

from fastapi import HTTPException
from pydantic import BaseModel, Field

from app.parsers import _parse_local, OfficeArchiveTooLargeError, UnsupportedFileTypeError



def _max_attachment_bytes() -> int:
    """The size cap of one attachment, 1 MB until the operator raises it.

    The same number the model-gateway attachment policy declares as
    `max_file_bytes` (`CHAT_ATTACHMENT_MAX_FILE_BYTES`), so raising the cap is one
    value in both places. Only a positive whole number counts; anything else keeps
    the default, so a typo never opens the limit.
    """
    raw = os.getenv("CHAT_EXTRACT_MAX_BYTES", "").strip()
    return int(raw) if raw.isdecimal() and int(raw) > 0 else 1_000_000


_MAX_BYTES = _max_attachment_bytes()
# Base64 is 4/3 of the bytes; the rest is room for padding and a data-URL prefix.
_MAX_BASE64_CHARS = _MAX_BYTES * 7 // 5
# One chat turn takes at most this much text per document, whatever the file size.
_MAX_CHARS = 60_000
# A workbook is cut on a row boundary a little below that, so the formula list
# and the cut markers still fit inside the limit.
_XLSX_MAX_CHARS = 55_000


class ChatExtractRequest(BaseModel):
    filename: str = Field(min_length=1, max_length=255)
    content_base64: str = Field(min_length=1, max_length=_MAX_BASE64_CHARS)
    content_type: str | None = Field(default=None, max_length=128)


def extract_chat_document(request: ChatExtractRequest) -> dict:
    try:
        content = base64.b64decode(request.content_base64, validate=True)
    except (ValueError, binascii.Error) as exc:
        raise HTTPException(400, "Invalid attachment encoding") from exc
    if not content or len(content) > _MAX_BYTES:
        raise HTTPException(413, f"Choose a non-empty attachment smaller than {_MAX_BYTES / 1_000_000:g} MB")
    metadata: dict = {}
    try:
        text = _parse_local(
            Path(request.filename).suffix.lower().lstrip('.'), content, metadata, max_chars=_XLSX_MAX_CHARS,
        )
    except UnsupportedFileTypeError as exc:
        raise HTTPException(415, "This attachment type is not supported") from exc
    except OfficeArchiveTooLargeError as exc:
        raise HTTPException(413, "The attachment expands to too much content to read safely. Choose a smaller file.") from exc
    except Exception as exc:
        # Parser exceptions may contain document text. Do not expose or log them.
        raise HTTPException(422, "The attachment could not be read. Check the file and retry.") from exc
    if not text.strip():
        raise HTTPException(422, "No readable text found. Scanned files require a text version.")
    if len(text) > _MAX_CHARS:
        raise HTTPException(413, "The document is too long for this turn. Choose a smaller excerpt.")
    data = {"name": request.filename, "content": text, "persisted": False}
    if metadata.get("truncated"):
        # The sheet was cut on a row boundary; the text says where.
        data["truncated"] = True
    return {"data": data}
