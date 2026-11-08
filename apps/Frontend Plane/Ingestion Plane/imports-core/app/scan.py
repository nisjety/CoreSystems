"""Upload scan seam for room files (WS14-S6).

Room files keep only extracted text, never the original bytes, so there is no
file to serve back and the scan engine itself belongs to the original-bytes
store (epic E1). This module is the one place that engine plugs in: every room
upload passes through `scan_upload` before it is parsed, and the verdict is
stamped on the document as `scan_state`.

Today the only state is `not_scanned_text_only`: nothing is executed or served,
and the text layer is all that persists. A future engine returns
`ScanResult(clean=False, ...)` to refuse the file, or `state="clean"`.
"""

from dataclasses import dataclass

SCAN_STATE_TEXT_ONLY = "not_scanned_text_only"


@dataclass(frozen=True)
class ScanResult:
    clean: bool
    state: str
    reason: str | None = None


def scan_upload(filename: str, content: bytes, content_type: str | None) -> ScanResult:
    """Return the scan verdict for one uploaded file. Never logs the filename."""
    return ScanResult(clean=True, state=SCAN_STATE_TEXT_ONLY)
