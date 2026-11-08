import base64
import io
import logging
import shutil
import subprocess
from pathlib import Path

import pytest
from fastapi import HTTPException
from pypdf import PdfWriter

from app import ocr
from app.chat_extract import ChatExtractRequest, extract_chat_document

NO_TEXT = "No readable text found. Scanned files require a text version."
UNREADABLE = "The scanned file could not be read. Try a text version."
SAMPLE_SCAN = Path(__file__).parent / "fixtures" / "scanned-sample.pdf"


def request(name, content):
    return ChatExtractRequest(filename=name, content_base64=base64.b64encode(content).decode())


def blank_pdf(pages):
    """A PDF with no text layer, which is what pypdf sees in a scanned document."""
    writer = PdfWriter()
    for _ in range(pages):
        writer.add_blank_page(200, 200)
    buffer = io.BytesIO()
    writer.write(buffer)
    return buffer.getvalue()


def cut_marker(read, total):
    return f"\n\n[Read the first {read} of {total} pages. The rest was not read.]"


def _must_not_run(argv, timeout):
    raise AssertionError("a binary was started when it must not be")


class FakeBinaries:
    """Stands in for pdftoppm and tesseract at ocr._run, and records each call."""

    def __init__(self):
        self.text = "Scanned hello"
        self.timeout_pages = set()
        self.calls = []
        self.workdirs = []

    def __call__(self, argv, timeout):
        self.calls.append(list(argv))
        if argv[0] == "pdftoppm":
            self.workdirs.append(Path(argv[-1]).parent)
            page = int(argv[argv.index("-f") + 1])
            if page in self.timeout_pages:
                raise subprocess.TimeoutExpired(argv, timeout)
            Path(argv[-1] + ".png").write_bytes(b"\x89PNG")
            return subprocess.CompletedProcess(argv, 0, b"", b"")
        return subprocess.CompletedProcess(argv, 0, self.text.encode(), b"")

    def rendered_pages(self):
        return [call[call.index("-f") + 1] for call in self.calls if call[0] == "pdftoppm"]


@pytest.fixture
def binaries(monkeypatch):
    """The switch on, with pdftoppm and tesseract replaced by a FakeBinaries."""
    monkeypatch.setenv("CHAT_EXTRACT_OCR_ENABLED", "true")
    monkeypatch.delenv("CHAT_EXTRACT_OCR_MAX_PAGES", raising=False)
    fake = FakeBinaries()
    monkeypatch.setattr(ocr, "_run", fake)
    return fake


@pytest.mark.parametrize("value", [None, "TRUE", "1", " true", "yes"])
def test_the_switch_is_on_only_for_the_literal_true(monkeypatch, value):
    if value is None:
        monkeypatch.delenv("CHAT_EXTRACT_OCR_ENABLED", raising=False)
    else:
        monkeypatch.setenv("CHAT_EXTRACT_OCR_ENABLED", value)
    monkeypatch.setattr(ocr, "_run", _must_not_run)
    with pytest.raises(HTTPException) as exc:
        extract_chat_document(request("scan.pdf", blank_pdf(1)))
    assert exc.value.status_code == 422
    assert exc.value.detail == NO_TEXT


def test_a_scanned_pdf_is_read_locally_and_marked_as_ocr(binaries):
    result = extract_chat_document(request("scan.pdf", blank_pdf(1)))
    assert result == {"data": {"name": "scan.pdf", "content": "Scanned hello", "persisted": False, "ocr": True}}
    assert binaries.rendered_pages() == ["1"]


def test_pages_past_the_cap_are_not_read_and_the_text_says_so(binaries, monkeypatch):
    monkeypatch.setenv("CHAT_EXTRACT_OCR_MAX_PAGES", "2")
    data = extract_chat_document(request("scan.pdf", blank_pdf(3)))["data"]
    assert data["content"] == "Scanned hello\n\f\nScanned hello" + cut_marker(2, 3)
    assert data["truncated"] is True
    assert binaries.rendered_pages() == ["1", "2"]


def test_a_timeout_on_the_first_page_is_unreadable_and_leaves_no_files(binaries):
    binaries.timeout_pages = {1}
    with pytest.raises(HTTPException) as exc:
        extract_chat_document(request("scan.pdf", blank_pdf(2)))
    assert exc.value.status_code == 422
    assert exc.value.detail == UNREADABLE
    assert binaries.workdirs
    assert not any(path.exists() for path in binaries.workdirs)


def test_a_timeout_on_a_later_page_keeps_the_pages_already_read(binaries):
    binaries.timeout_pages = {2}
    data = extract_chat_document(request("scan.pdf", blank_pdf(3)))["data"]
    assert data["content"] == "Scanned hello" + cut_marker(1, 3)
    assert data["truncated"] is True
    assert data["ocr"] is True


def test_the_deadline_stops_after_the_page_that_crosses_it(binaries, monkeypatch):
    clock = {"now": 0.0}

    def first_page_uses_the_whole_budget(argv, timeout):
        result = binaries(argv, timeout)
        if argv[0] == "tesseract":
            clock["now"] = ocr.DEADLINE_S + 1.0
        return result

    monkeypatch.setattr(ocr, "_run", first_page_uses_the_whole_budget)
    monkeypatch.setattr(ocr.time, "monotonic", lambda: clock["now"])
    data = extract_chat_document(request("scan.pdf", blank_pdf(3)))["data"]
    assert data["content"] == "Scanned hello" + cut_marker(1, 3)
    assert data["truncated"] is True
    assert binaries.rendered_pages() == ["1"]


def test_ocr_text_over_the_turn_limit_is_refused_with_413(binaries):
    binaries.text = "x" * 60_001
    with pytest.raises(HTTPException) as exc:
        extract_chat_document(request("scan.pdf", blank_pdf(1)))
    assert exc.value.status_code == 413


def test_ocr_text_never_reaches_the_log(binaries, caplog):
    binaries.text = "HEMMELIG-OCR-TEKST"
    caplog.set_level(logging.DEBUG)
    data = extract_chat_document(request("scan.pdf", blank_pdf(1)))["data"]
    assert data["content"] == "HEMMELIG-OCR-TEKST"
    assert "HEMMELIG" not in caplog.text
    assert len([record for record in caplog.records if record.name == "app.ocr"]) <= 1


@pytest.mark.parametrize(
    "raw,expected",
    [("0", 15), ("-1", 15), ("51", 15), ("abc", 15), ("", 15), (" 20", 15), ("2.5", 15),
     ("1", 1), ("20", 20), ("50", 50)],
)
def test_the_page_cap_is_a_whole_number_from_1_to_50(monkeypatch, raw, expected):
    monkeypatch.setenv("CHAT_EXTRACT_OCR_MAX_PAGES", raw)
    assert ocr.max_pages() == expected


def test_the_page_cap_is_15_when_unset(monkeypatch):
    monkeypatch.delenv("CHAT_EXTRACT_OCR_MAX_PAGES", raising=False)
    assert ocr.max_pages() == 15


def test_a_scan_that_reads_as_blank_keeps_the_no_text_message(binaries):
    binaries.text = "  \n"
    with pytest.raises(HTTPException) as exc:
        extract_chat_document(request("scan.pdf", blank_pdf(1)))
    assert exc.value.status_code == 422
    assert exc.value.detail == NO_TEXT


def test_only_a_pdf_is_sent_to_ocr(binaries, monkeypatch):
    monkeypatch.setattr(ocr, "_run", _must_not_run)
    with pytest.raises(HTTPException) as exc:
        extract_chat_document(request("empty.md", b" "))
    assert exc.value.status_code == 422
    assert exc.value.detail == NO_TEXT


def test_a_missing_binary_is_unreadable_not_a_server_error(monkeypatch):
    monkeypatch.setenv("CHAT_EXTRACT_OCR_ENABLED", "true")

    def no_binary(argv, timeout):
        raise FileNotFoundError(argv[0])

    monkeypatch.setattr(ocr, "_run", no_binary)
    with pytest.raises(HTTPException) as exc:
        extract_chat_document(request("scan.pdf", blank_pdf(1)))
    assert exc.value.status_code == 422
    assert exc.value.detail == UNREADABLE


def test_a_busy_ocr_slot_is_unavailable_instead_of_waiting(monkeypatch):
    monkeypatch.setattr(ocr, "SLOT_WAIT_S", 0.01)
    monkeypatch.setattr(ocr, "_run", _must_not_run)
    assert ocr._SLOT.acquire(timeout=1)
    try:
        with pytest.raises(ocr.OcrUnavailableError):
            ocr.ocr_pdf(blank_pdf(1))
    finally:
        ocr._SLOT.release()


@pytest.mark.skipif(
    not (shutil.which("tesseract") and shutil.which("pdftoppm") and SAMPLE_SCAN.exists()),
    reason="needs tesseract, pdftoppm and tests/fixtures/scanned-sample.pdf",
)
def test_real_tesseract_and_poppler_read_a_scanned_sample(monkeypatch):
    monkeypatch.setenv("CHAT_EXTRACT_OCR_ENABLED", "true")
    data = extract_chat_document(request("scan.pdf", SAMPLE_SCAN.read_bytes()))["data"]
    assert data["ocr"] is True
    assert data["content"].strip()
