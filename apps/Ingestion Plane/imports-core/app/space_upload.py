"""Room-file upload validation and typed refusals (WS14-S6).

`POST /api/v1/import/jobs/space-upload` accepts documents for a Space under a
Control-issued `space_upload` import decision. This module owns everything that
happens before a job exists: the refusal catalogue, the limits, and the
per-file validate + extract step. Only extracted text is kept; the original
bytes are never stored (WS14 decision D-4).

Refusals use the typed envelope `{error:{code,message,details}}`. Messages never
carry a file name or any document text.
"""

import logging
from pathlib import Path

from fastapi import UploadFile
from fastapi.responses import JSONResponse
from starlette.concurrency import run_in_threadpool

from app.config import Settings
from app.parsers import OfficeArchiveTooLargeError, UnsupportedFileTypeError, parse_uploaded_file
from app.scan import scan_upload
from app.schemas import ImportDocument

logger = logging.getLogger(__name__)

SPACE_UPLOAD_SOURCE_TYPE = "space_upload"
# Extracted text per file (5 MiB), whatever the file size.
MAX_TEXT_BYTES = 5_242_880

_MESSAGES = {
    "room_files_disabled": "Room files are not enabled.",
    "room_file_empty": "Choose a file that is not empty.",
    "room_file_count_exceeded": "Too many files in one upload.",
    "room_file_too_large": "The file is larger than the room file limit.",
    "room_file_type_unsupported": "This file type cannot be added to a room.",
    "room_file_no_extractable_text": "No readable text was found in the file. Scanned files need a text version.",
    "room_file_text_too_long": "The file holds more text than a room file can.",
    "room_files_quota_exceeded": "The import quota is used up.",
    "space_files_not_authorized": "Adding files to this room is not authorized.",
    "zdr_durable_mutation_forbidden": "Zero Data Retention credentials cannot create, change, or delete durable state.",
}


class SpaceUploadRefusal(Exception):
    def __init__(self, status: int, code: str, details: dict | None = None) -> None:
        super().__init__(code)
        self.status = status
        self.code = code
        self.details = details or {}


def refusal_response(refusal: SpaceUploadRefusal) -> JSONResponse:
    return JSONResponse(
        status_code=refusal.status,
        content={
            "error": {
                "code": refusal.code,
                "message": _MESSAGES.get(refusal.code, "The upload was refused."),
                "details": refusal.details,
            }
        },
        headers={"Cache-Control": "no-store"},
    )


def space_upload_enabled(settings: Settings) -> bool:
    return settings.imports_space_upload.strip() == "true"


async def prepare_space_upload(
    files: list[UploadFile], settings: Settings
) -> list[ImportDocument]:
    """Validate every file and return the extracted documents, or raise a refusal.

    Nothing is persisted here. Parser exceptions can carry document text, so only
    their type is ever logged.
    """
    if not files:
        raise SpaceUploadRefusal(400, "room_file_empty")
    if len(files) > settings.space_upload_max_files:
        raise SpaceUploadRefusal(
            400, "room_file_count_exceeded", {"max_files": settings.space_upload_max_files}
        )

    allowed = settings.space_upload_allowed_types
    max_bytes = settings.space_upload_max_file_mb * 1024 * 1024
    documents: list[ImportDocument] = []
    for upload in files:
        name = Path(upload.filename or "").name
        extension = Path(name).suffix.lower().lstrip(".")
        if extension not in allowed:
            raise SpaceUploadRefusal(415, "room_file_type_unsupported", {"allowed": allowed})

        content = await upload.read(max_bytes + 1)
        if not content:
            raise SpaceUploadRefusal(400, "room_file_empty")
        if len(content) > max_bytes:
            raise SpaceUploadRefusal(413, "room_file_too_large", {"max_bytes": max_bytes})

        scan = scan_upload(name, content, upload.content_type)
        if not scan.clean:
            raise SpaceUploadRefusal(
                415, "room_file_type_unsupported", {"allowed": allowed, "reason": "scan_rejected"}
            )

        try:
            document = await run_in_threadpool(
                parse_uploaded_file, name, content, upload.content_type
            )
        except OfficeArchiveTooLargeError as exc:
            raise SpaceUploadRefusal(
                413, "room_file_text_too_long", {"max_bytes": MAX_TEXT_BYTES}
            ) from exc
        except UnsupportedFileTypeError as exc:
            raise SpaceUploadRefusal(415, "room_file_type_unsupported", {"allowed": allowed}) from exc
        except Exception as exc:
            logger.warning("space-upload parse failed error_type=%s", type(exc).__name__)
            raise SpaceUploadRefusal(
                422, "room_file_no_extractable_text", {"reason": "unreadable"}
            ) from exc

        text = document.text or ""
        if not text.strip():
            raise SpaceUploadRefusal(422, "room_file_no_extractable_text")
        if len(text.encode("utf-8")) > MAX_TEXT_BYTES:
            raise SpaceUploadRefusal(413, "room_file_text_too_long", {"max_bytes": MAX_TEXT_BYTES})

        document.metadata.update(
            {
                "text_chars": len(text),
                "created_by_kind": "human",
                "scan_state": scan.state,
            }
        )
        documents.append(document)
    return documents


def item_error_code(status_code: int | None, body: object) -> str:
    """Map a documents-api failure for one room file to a stable item error code."""
    if isinstance(body, dict):
        code = body.get("code")
        if isinstance(code, str) and code:
            return code
    if status_code == 404:
        return "room_files_disabled"
    return "room_file_store_failed"
