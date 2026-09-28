"""Bounded, local-only document extraction. No jobs, database or external parser."""
import base64
import binascii
from pathlib import Path

from fastapi import HTTPException
from pydantic import BaseModel, Field

from app.parsers import _parse_local, UnsupportedFileTypeError


class ChatExtractRequest(BaseModel):
    filename: str = Field(min_length=1, max_length=255)
    content_base64: str = Field(min_length=1, max_length=1_400_000)
    content_type: str | None = Field(default=None, max_length=128)


def extract_chat_document(request: ChatExtractRequest) -> dict:
    try:
        content = base64.b64decode(request.content_base64, validate=True)
    except (ValueError, binascii.Error) as exc:
        raise HTTPException(400, "Invalid attachment encoding") from exc
    if not content or len(content) > 1_000_000:
        raise HTTPException(413, "Choose a non-empty attachment smaller than 1 MB")
    try:
        text = _parse_local(Path(request.filename).suffix.lower().lstrip('.'), content, {})
    except UnsupportedFileTypeError as exc:
        raise HTTPException(415, "This attachment type is not supported") from exc
    except Exception as exc:
        # Parser exceptions may contain document text. Do not expose or log them.
        raise HTTPException(422, "The attachment could not be read. Check the file and retry.") from exc
    if not text.strip():
        raise HTTPException(422, "No readable text found. Scanned files require a text version.")
    if len(text) > 60_000:
        raise HTTPException(413, "The document is too long for this turn. Choose a smaller excerpt.")
    return {"data": {"name": request.filename, "content": text, "persisted": False}}
