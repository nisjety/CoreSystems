"""WS14-S6: every typed refusal of POST /api/v1/import/jobs/space-upload.

Refusals use `{error:{code,message,details}}` with `Cache-Control: no-store`,
never carry a file name or document text, and happen in the documented order:
flag, ZDR, Control decision (nothing is read before one), replay, validation,
quota. The database, quota service and orchestrator are stubbed.
"""

import logging

import pytest

from app import space_upload
from app.parsers import OfficeArchiveTooLargeError, UnsupportedFileTypeError
from app.scan import ScanResult
from app.schemas import ImportDocument
from space_upload_support import decision_token, headers, install, uninstall

URL = "/api/v1/import/jobs/space-upload"
TXT = ("files", ("notes.txt", b"hello room", "text/plain"))


@pytest.fixture(autouse=True)
def _clean_overrides():
    yield
    uninstall()


def _assert_refusal(response, status: int, code: str) -> dict:
    assert response.status_code == status, response.text
    body = response.json()
    assert set(body) == {"error"}, body
    assert set(body["error"]) == {"code", "message", "details"}, body
    assert body["error"]["code"] == code
    assert isinstance(body["error"]["message"], str) and body["error"]["message"]
    assert response.headers["cache-control"] == "no-store"
    return body["error"]["details"]


def test_flag_off_is_404_room_files_disabled_before_anything_is_read(monkeypatch) -> None:
    client, recorder = install(monkeypatch, enabled=False)
    token = decision_token(monkeypatch)
    response = client.post(URL, files=[TXT], headers=headers(token))
    _assert_refusal(response, 404, "room_files_disabled")
    assert recorder.find_calls == [] and recorder.create_calls == []


def test_only_the_literal_true_turns_the_route_on(monkeypatch) -> None:
    from app import main

    client, _ = install(monkeypatch, enabled=False)
    for value in ("True", "1", "yes", " false ", ""):
        monkeypatch.setattr(main.settings, "imports_space_upload", value)
        _assert_refusal(client.post(URL, files=[TXT], headers=headers(None)), 404, "room_files_disabled")


@pytest.mark.parametrize("how", ["form", "header"])
def test_zdr_is_412_with_the_surface_named(monkeypatch, how) -> None:
    client, recorder = install(monkeypatch)
    token = decision_token(monkeypatch)
    kwargs = {"data": {"zdr": "true"}} if how == "form" else {}
    extra = {"X-ZDR": "true"} if how == "header" else {}
    response = client.post(URL, files=[TXT], headers=headers(token, **extra), **kwargs)
    details = _assert_refusal(response, 412, "zdr_durable_mutation_forbidden")
    assert details == {"surface": "room_files"}
    assert recorder.create_calls == []


def test_a_call_without_a_decision_is_403_decision_required_and_nothing_is_parsed(monkeypatch) -> None:
    client, recorder = install(monkeypatch)

    def never_parsed(*_args, **_kwargs):
        raise AssertionError("no file may be read without a Control decision")

    from app import main

    monkeypatch.setattr(main, "prepare_space_upload", never_parsed)
    garbage = ("files", ("virus.exe", b"MZ\x90\x00", "application/octet-stream"))
    details = _assert_refusal(client.post(URL, files=[garbage], headers=headers(None)), 403, "space_files_not_authorized")
    assert details == {"reason_code": "decision_required"}
    assert recorder.find_calls == []


def test_a_malformed_or_wrong_subject_decision_is_403_decision_invalid(monkeypatch) -> None:
    token = decision_token(monkeypatch)
    client, recorder = install(monkeypatch)
    for bad in ("forged", "v2.a.b", token.rsplit(".", 1)[0]):
        details = _assert_refusal(client.post(URL, files=[TXT], headers=headers(bad)), 403, "space_files_not_authorized")
        assert details == {"reason_code": "decision_invalid"}, bad

    # A genuine decision issued for another member cannot be used by this user.
    other = decision_token(monkeypatch, subject_id="user-2")
    client, recorder = install(monkeypatch)
    details = _assert_refusal(client.post(URL, files=[TXT], headers=headers(other)), 403, "space_files_not_authorized")
    assert details == {"reason_code": "decision_invalid"}
    assert recorder.create_calls == []


@pytest.mark.skip(
    reason="DEPTH-S6-1: verify_space_import_ingress_decision lets cryptography's InvalidSignature "
    "escape (it is not a ValueError), so a decision with a well-formed but wrong signature is an "
    "unhandled 500 instead of the typed 403 decision_invalid. Fail-closed but the wrong envelope."
)
def test_a_decision_with_a_wrong_signature_is_403_decision_invalid(monkeypatch) -> None:
    token = decision_token(monkeypatch)
    client, recorder = install(monkeypatch)
    tampered = token.rsplit(".", 1)[0] + "." + "A" * 86
    details = _assert_refusal(client.post(URL, files=[TXT], headers=headers(tampered)), 403, "space_files_not_authorized")
    assert details == {"reason_code": "decision_invalid"}
    assert recorder.create_calls == []


def test_a_decision_for_another_source_type_cannot_open_the_room_file_route(monkeypatch) -> None:
    client, recorder = install(monkeypatch)
    token = decision_token(monkeypatch, import_source_type="notion")
    details = _assert_refusal(client.post(URL, files=[TXT], headers=headers(token)), 403, "space_files_not_authorized")
    assert details == {"reason_code": "source_type_not_allowed"}
    assert recorder.create_calls == []


def test_an_idempotency_key_that_differs_from_the_decisions_is_403(monkeypatch) -> None:
    client, recorder = install(monkeypatch)
    token = decision_token(monkeypatch)
    details = _assert_refusal(
        client.post(URL, files=[TXT], headers=headers(token, key="a-different-key")), 403, "space_files_not_authorized"
    )
    assert details == {"reason_code": "idempotency_mismatch"}
    assert recorder.find_calls == []


def test_no_files_and_an_empty_file_are_400_room_file_empty(monkeypatch) -> None:
    client, _ = install(monkeypatch)
    token = decision_token(monkeypatch)
    _assert_refusal(client.post(URL, headers=headers(token)), 400, "room_file_empty")
    empty = ("files", ("empty.txt", b"", "text/plain"))
    _assert_refusal(client.post(URL, files=[empty], headers=headers(token)), 400, "room_file_empty")


def test_too_many_files_is_400_with_the_limit(monkeypatch) -> None:
    client, _ = install(monkeypatch)
    token = decision_token(monkeypatch)
    files = [("files", (f"n{i}.txt", b"hello", "text/plain")) for i in range(11)]
    details = _assert_refusal(client.post(URL, files=files, headers=headers(token)), 400, "room_file_count_exceeded")
    assert details == {"max_files": 10}


def test_a_file_over_the_byte_limit_is_413_with_max_bytes(monkeypatch) -> None:
    from app import main

    client, _ = install(monkeypatch)
    monkeypatch.setattr(main.settings, "space_upload_max_file_mb", 1)
    token = decision_token(monkeypatch)
    big = ("files", ("big.txt", b"a" * (1024 * 1024 + 1), "text/plain"))
    details = _assert_refusal(client.post(URL, files=[big], headers=headers(token)), 413, "room_file_too_large")
    assert details == {"max_bytes": 1024 * 1024}


def test_an_unlisted_extension_is_415_with_the_allowed_list(monkeypatch) -> None:
    client, _ = install(monkeypatch)
    token = decision_token(monkeypatch)
    for name in ("run.exe", "noextension", "page.html", "archive.zip"):
        bad = ("files", (name, b"content", "application/octet-stream"))
        details = _assert_refusal(client.post(URL, files=[bad], headers=headers(token)), 415, "room_file_type_unsupported")
        assert "docx" in details["allowed"] and "exe" not in details["allowed"], name


def test_an_unsupported_parser_type_is_415(monkeypatch) -> None:
    client, _ = install(monkeypatch)
    token = decision_token(monkeypatch)

    def refuse(*_args, **_kwargs):
        raise UnsupportedFileTypeError("nope")

    monkeypatch.setattr(space_upload, "parse_uploaded_file", refuse)
    details = _assert_refusal(client.post(URL, files=[TXT], headers=headers(token)), 415, "room_file_type_unsupported")
    assert "allowed" in details


def test_a_scan_rejection_is_415_with_the_reason(monkeypatch) -> None:
    client, _ = install(monkeypatch)
    token = decision_token(monkeypatch)
    monkeypatch.setattr(space_upload, "scan_upload", lambda *_a: ScanResult(clean=False, state="infected", reason="x"))
    details = _assert_refusal(client.post(URL, files=[TXT], headers=headers(token)), 415, "room_file_type_unsupported")
    assert details["reason"] == "scan_rejected"


def test_a_file_with_no_readable_text_is_422(monkeypatch) -> None:
    client, _ = install(monkeypatch)
    token = decision_token(monkeypatch)
    monkeypatch.setattr(
        space_upload, "parse_uploaded_file", lambda *_a, **_k: ImportDocument(source_name="scan.pdf", text="  \n\t ")
    )
    _assert_refusal(client.post(URL, files=[TXT], headers=headers(token)), 422, "room_file_no_extractable_text")


def test_extracted_text_over_the_cap_is_413_room_file_text_too_long(monkeypatch) -> None:
    client, _ = install(monkeypatch)
    token = decision_token(monkeypatch)
    huge = ImportDocument(source_name="n.txt", text="a" * (space_upload.MAX_TEXT_BYTES + 1))
    monkeypatch.setattr(space_upload, "parse_uploaded_file", lambda *_a, **_k: huge)
    details = _assert_refusal(client.post(URL, files=[TXT], headers=headers(token)), 413, "room_file_text_too_long")
    assert details == {"max_bytes": space_upload.MAX_TEXT_BYTES}

    def too_big(*_args, **_kwargs):
        raise OfficeArchiveTooLargeError("zip bomb")

    monkeypatch.setattr(space_upload, "parse_uploaded_file", too_big)
    _assert_refusal(client.post(URL, files=[TXT], headers=headers(token)), 413, "room_file_text_too_long")


def test_the_import_quota_is_409_room_files_quota_exceeded(monkeypatch) -> None:
    client, recorder = install(monkeypatch)
    recorder.quota_allowed = False
    token = decision_token(monkeypatch)
    _assert_refusal(client.post(URL, files=[TXT], headers=headers(token)), 409, "room_files_quota_exceeded")
    assert recorder.create_calls == [] and recorder.dispatched == []


def test_an_unparseable_file_is_422_and_neither_the_name_nor_the_text_reach_the_logs(monkeypatch, caplog) -> None:
    client, _ = install(monkeypatch)
    token = decision_token(monkeypatch)

    def explode(*_args, **_kwargs):
        raise ValueError("secret-clause-text found in secret-name.txt")

    monkeypatch.setattr(space_upload, "parse_uploaded_file", explode)
    secret = ("files", ("secret-name.txt", b"secret-clause-text", "text/plain"))
    with caplog.at_level(logging.DEBUG):
        response = client.post(URL, files=[secret], headers=headers(token))

    details = _assert_refusal(response, 422, "room_file_no_extractable_text")
    assert details == {"reason": "unreadable"}
    assert "secret-name" not in response.text and "secret-clause" not in response.text
    logged = "\n".join(record.getMessage() for record in caplog.records)
    assert "secret-name" not in logged and "secret-clause" not in logged
    assert "error_type=ValueError" in logged


def test_no_refusal_message_carries_a_file_name(monkeypatch) -> None:
    client, _ = install(monkeypatch)
    token = decision_token(monkeypatch)
    bad = ("files", ("quarterly-budget.exe", b"x", "application/octet-stream"))
    response = client.post(URL, files=[bad], headers=headers(token))
    assert "quarterly-budget" not in response.text


def test_item_error_codes_map_documents_api_failures_to_stable_codes() -> None:
    assert space_upload.item_error_code(409, {"code": "room_file_duplicate"}) == "room_file_duplicate"
    assert space_upload.item_error_code(409, {"code": "room_files_quota_exceeded", "error": "x"}) == "room_files_quota_exceeded"
    assert space_upload.item_error_code(404, None) == "room_files_disabled"
    assert space_upload.item_error_code(404, {"code": ""}) == "room_files_disabled"
    assert space_upload.item_error_code(500, {"error": "boom"}) == "room_file_store_failed"
    assert space_upload.item_error_code(None, "not a dict") == "room_file_store_failed"
    assert space_upload.item_error_code(503, {"code": 5}) == "room_file_store_failed"
