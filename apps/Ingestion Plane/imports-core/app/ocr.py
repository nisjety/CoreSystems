"""Local OCR for scanned PDFs on the ephemeral chat extract route.

Tesseract and poppler run inside this container. Nothing goes over the network,
nothing is kept beyond one temporary directory, and neither the document text
nor a binary's output is written to a log line or an exception. The switch is
CHAT_EXTRACT_OCR_ENABLED, and only the literal "true" turns it on.
"""

import io
import logging
import os
import re
import subprocess
import tempfile
import threading
import time
from typing import NamedTuple

from pypdf import PdfReader

_log = logging.getLogger(__name__)

# CHATGAP OD18: the whole request, including the wait for the slot, stays under
# the BFF's 25 s client timeout.
DEADLINE_S = 18.0
PAGE_TIMEOUT_S = 10.0
# imports-api has 256 MB and a quarter of a CPU, so one document is read at a
# time. A caller that waits longer than this is told the file is unavailable.
SLOT_WAIT_S = 2.0
_SLOT = threading.BoundedSemaphore(1)
_DEFAULT_MAX_PAGES = 15
_MAX_PAGES_LIMIT = 50
_WHOLE_NUMBER = re.compile(r"[0-9]{1,3}")


class OcrUnavailableError(Exception):
    """OCR could not read the document. The message never contains document content."""


class OcrResult(NamedTuple):
    text: str
    pages_read: int
    pages_total: int


def ocr_enabled() -> bool:
    # Read on every call, so no value is fixed when the module is imported.
    return os.getenv("CHAT_EXTRACT_OCR_ENABLED") == "true"


def max_pages() -> int:
    """CHAT_EXTRACT_OCR_MAX_PAGES: a whole number from 1 to 50. Anything else gives 15."""
    raw = os.getenv("CHAT_EXTRACT_OCR_MAX_PAGES")
    if raw is not None and _WHOLE_NUMBER.fullmatch(raw):
        value = int(raw)
        if 1 <= value <= _MAX_PAGES_LIMIT:
            return value
    return _DEFAULT_MAX_PAGES


def ocr_pdf(content: bytes) -> OcrResult:
    """Read the pages of a scanned PDF with local Tesseract.

    Raises OcrUnavailableError when no page can be read. Otherwise returns the
    text of the pages read, in order. Fewer pages than the PDF has means that the
    page cap or the deadline stopped the read.
    """
    started = time.monotonic()
    deadline = started + DEADLINE_S
    if not _SLOT.acquire(timeout=SLOT_WAIT_S):
        raise OcrUnavailableError("OCR is busy with another document.")
    try:
        result = _read_pages(content, deadline)
    finally:
        _SLOT.release()
    _log.info(
        "chat extract OCR: pages_read=%d pages_total=%d elapsed_ms=%d",
        result.pages_read,
        result.pages_total,
        int((time.monotonic() - started) * 1000),
    )
    return result


def _read_pages(content: bytes, deadline: float) -> OcrResult:
    try:
        total = len(PdfReader(io.BytesIO(content)).pages)
    except Exception:
        # pypdf's message can quote parts of the file, so it is not kept.
        raise OcrUnavailableError("The PDF could not be opened for OCR.") from None
    pages: list[str] = []
    try:
        with tempfile.TemporaryDirectory(prefix="ocr-") as work:
            source = os.path.join(work, "in.pdf")
            with open(source, "wb") as handle:
                handle.write(content)
            for index in range(1, min(total, max_pages()) + 1):
                if time.monotonic() >= deadline:
                    break
                text = _page_text(work, source, index, deadline)
                if text is None:
                    if index == 1:
                        raise OcrUnavailableError("OCR could not read the first page.")
                    # A later page that fails keeps the pages already read.
                    break
                pages.append(text)
    except OSError:
        raise OcrUnavailableError("OCR could not use its work directory.") from None
    if not pages:
        raise OcrUnavailableError("OCR read no pages before the deadline.")
    return OcrResult("\n\f\n".join(pages).strip(), len(pages), total)


def _budget(deadline: float) -> float:
    return min(PAGE_TIMEOUT_S, deadline - time.monotonic())


def _page_text(work: str, source: str, index: int, deadline: float) -> str | None:
    """The text of one page, or None if it could not be rendered or read in time.

    The exception and the binary's stderr are dropped on purpose. Stderr can
    quote document text, and nothing from a binary may reach a log or an error.
    """
    stem = os.path.join(work, f"p{index}")
    try:
        budget = _budget(deadline)
        if budget <= 0:
            return None
        rendered = _run(
            ["pdftoppm", "-r", "200", "-gray", "-png", "-f", str(index), "-l", str(index), "-singlefile", source, stem],
            budget,
        )
        budget = _budget(deadline)
        if rendered.returncode != 0 or budget <= 0:
            return None
        read = _run(["tesseract", stem + ".png", "stdout", "-l", "eng+nor", "--psm", "3"], budget)
    except (OSError, subprocess.SubprocessError):
        return None
    if read.returncode != 0:
        return None
    return read.stdout.decode("utf-8", errors="replace").strip()


def _run(argv: list[str], timeout: float) -> subprocess.CompletedProcess:
    """The one place a binary is started. Tests replace this seam and nothing else."""
    return subprocess.run(
        argv,
        shell=False,
        capture_output=True,
        timeout=timeout,
        env={"PATH": os.environ.get("PATH", ""), "OMP_THREAD_LIMIT": "1"},
        check=False,
    )
