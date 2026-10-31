"""Unit tests for the stable-prefix A/B driver and comparison (WS02-S8).

Nothing here reaches a network: the traffic is a fake sender, the analytics
fetch goes through an httpx mock transport, and the routes are read from the
source tree. The live run is the operator's, through the runbook.
"""

from __future__ import annotations

import json
from dataclasses import asdict
from pathlib import Path

import httpx
import pytest

from eval_lab.report import render_report
from eval_lab.stable_prefix_ab import (
    ANALYTICS_FILE,
    ANALYTICS_PATH,
    DO_RUNS,
    THREADS,
    THREAD_TURNS,
    TURNS_FILE,
    WINDOW_FILE,
    ArmEvidence,
    EvalReport,
    TurnSummary,
    capture_analytics,
    compare_evidence,
    default_repo_root,
    drive_arm,
    gateway_sender,
    load_arm,
    main,
    parity_differences,
    parse_eval_report,
    plan_requests,
    record_turn,
    render_comparison,
    route_check,
    run_requests,
    summarize_analytics,
    summarize_turns,
)
from eval_lab.types import CaseResult, MetricOutcome, StreamEvent

TOTAL_REQUESTS = THREADS * len(THREAD_TURNS) + DO_RUNS


def _usage(provider: str = "openai", *, cache_read: int = 0, cache_write: int = 0) -> StreamEvent:
    # The same keys model-gateway writes: the cache keys appear only when set.
    data: dict[str, object] = {
        "input_tokens": 1000,
        "output_tokens": 40,
        "cost_usd": 0.001,
        "latency_ms": 900,
        "provider_used": provider,
    }
    if cache_read or cache_write:
        data["cache_read_tokens"] = cache_read
        data["cache_write_tokens"] = cache_write
    return StreamEvent(event="usage", data=data)


def _turn_events(text: str = "ok", provider: str = "openai") -> list[StreamEvent]:
    return [
        StreamEvent(event="connected", data={"request_id": "req_1", "ok": True}),
        StreamEvent(event="chunk", data={"delta": text}),
        _usage(provider),
        StreamEvent(event="done", data={"model_used": "gpt-5-mini"}),
    ]


def _bucket(
    provider: str,
    *,
    requests: int = 1,
    input_tokens: int = 0,
    read: int = 0,
    creation: int = 0,
    reported: int = 0,
) -> dict[str, object]:
    return {
        "day": "2026-10-24",
        "model": "gpt-5-mini",
        "provider": provider,
        "basis": "metered",
        "requests": requests,
        "input_tokens": input_tokens,
        "output_tokens": 0,
        "metered_cost_usd": 0.0,
        "unmeasured_requests": 0,
        "cache_read_input_tokens": read,
        "cache_creation_input_tokens": creation,
        "cache_reported_input_tokens": reported,
    }


def _analytics(
    buckets: list[dict[str, object]], misses: list[dict[str, object]] | None = None
) -> dict[str, object]:
    return {
        "data": {
            "buckets": buckets,
            "scope": "organization",
            "timezone": "UTC",
            "cache_breakdown": {"miss_reasons": misses or [], "advisory": True},
        }
    }


def _turns_payload(send_text: str = "ok") -> dict[str, object]:
    planned = plan_requests("openai", None, "run-a")
    records = run_requests(planned, lambda body: _turn_events(send_text))
    return {"arm": "off", "turns": [asdict(record) for record in records]}


def _write_arm(
    arm_dir: Path,
    analytics: dict[str, object],
    reports: dict[str, str],
) -> None:
    arm_dir.mkdir(parents=True)
    (arm_dir / ANALYTICS_FILE).write_text(json.dumps(analytics), encoding="utf-8")
    (arm_dir / TURNS_FILE).write_text(json.dumps(_turns_payload()), encoding="utf-8")
    for rerun, text in reports.items():
        rerun_dir = arm_dir / "eval" / rerun
        rerun_dir.mkdir(parents=True)
        (rerun_dir / "2026-10-24.md").write_text(text, encoding="utf-8")


def _half_passing_report() -> str:
    results = [
        CaseResult(
            case_id="01-ok",
            passed=True,
            metrics=[MetricOutcome(metric="accuracy", passed=True, score=1.0)],
        ),
        CaseResult(
            case_id="02-bad",
            passed=False,
            metrics=[
                MetricOutcome(metric="cost", passed=False, score=0.0, detail="over ceiling")
            ],
        ),
    ]
    return render_report(results)


def _arm(name: str, analytics: dict[str, object], evals: list[EvalReport] | None = None) -> ArmEvidence:
    return ArmEvidence(
        arm=name,
        cache=summarize_analytics(analytics),
        turns=TurnSummary(sent=TOTAL_REQUESTS, failed=0, off_pinned_route=0, do_without_error=DO_RUNS),
        evals=evals or [],
    )


# ── the traffic ──────────────────────────────────────────────────────────


def test_every_planned_request_pins_the_provider_and_names_its_profile() -> None:
    planned = plan_requests("azure-openai", "verevon-balance", "run-a")
    assert len(planned) == TOTAL_REQUESTS == 26
    for request in planned:
        assert request.body["provider"] == "azure-openai"
        assert request.body["model"] == "verevon-balance"
        assert request.body["zdr"] is False
    threads = [request for request in planned if request.kind == "thread"]
    runs = [request for request in planned if request.kind == "do"]
    assert len(threads) == THREADS * len(THREAD_TURNS) and len(runs) == DO_RUNS
    assert {request.body["profile"] for request in threads} == {"chat"}
    assert {request.body["profile"] for request in runs} == {"deployed_agent"}
    assert all("agentic" in request.body["features"] for request in runs)  # type: ignore[operator]


def test_each_thread_is_one_session_and_every_idempotency_key_is_unique() -> None:
    planned = plan_requests("openai", None, "run-a")
    sessions: dict[tuple[str, int], set[object]] = {}
    for request in planned:
        sessions.setdefault((request.kind, request.group), set()).add(request.body["session_key"])
    assert all(len(keys) == 1 for keys in sessions.values())
    assert len({next(iter(keys)) for keys in sessions.values()}) == THREADS + DO_RUNS
    keys = [request.body["idempotency_key"] for request in planned]
    assert len(set(keys)) == len(keys)
    assert all("model" not in request.body for request in planned)


def test_a_second_run_never_reuses_a_session_or_idempotency_key() -> None:
    first = {request.body["idempotency_key"] for request in plan_requests("openai", None, "run-a")}
    second = {request.body["idempotency_key"] for request in plan_requests("openai", None, "run-b")}
    assert first.isdisjoint(second)


def test_the_plan_refuses_a_provider_the_flag_does_not_reach() -> None:
    with pytest.raises(ValueError, match="provider must be one of"):
        plan_requests("anthropic", None, "run-a")


def test_a_turn_is_recorded_as_counts_and_never_as_text() -> None:
    request = plan_requests("openai", None, "run-a")[0]
    secret = "The answer names a customer."
    record = record_turn(
        request,
        [
            StreamEvent(event="connected", data={"request_id": "req_7", "ok": True}),
            StreamEvent(event="chunk", data={"delta": secret}),
            _usage(cache_read=900),
            StreamEvent(event="done", data={"model_used": "gpt-5-mini"}),
        ],
    )
    assert record.request_id == "req_7"
    assert record.provider_used == "openai" and record.pinned_route
    assert record.cache_read_tokens == 900 and record.cache_write_tokens == 0
    assert record.text_chars == len(secret)
    assert record.error is None
    assert secret not in json.dumps(asdict(record))


def test_an_error_event_is_recorded_by_its_code_and_its_message_is_dropped() -> None:
    request = plan_requests("openai", None, "run-a")[0]
    record = record_turn(
        request,
        [StreamEvent(event="error", data={"code": "upstream_timeout", "message": "private detail"})],
    )
    assert record.error == "upstream_timeout"
    assert "private detail" not in json.dumps(asdict(record))


def test_a_stream_that_ends_before_done_is_an_error() -> None:
    request = plan_requests("openai", None, "run-a")[0]
    record = record_turn(request, [StreamEvent(event="chunk", data={"delta": "partial"})])
    assert record.error == "stream ended before done"


def test_a_failed_turn_is_recorded_and_the_arm_carries_on() -> None:
    planned = plan_requests("openai", None, "run-a")
    calls: list[dict[str, object]] = []

    def send(body: dict[str, object]) -> list[StreamEvent]:
        calls.append(body)
        if len(calls) == 1:
            raise httpx.ConnectError("connection refused")
        return _turn_events()

    records = run_requests(planned, send)
    assert len(calls) == len(planned) == TOTAL_REQUESTS
    assert records[0].error == "ConnectError: connection refused"
    assert all(record.error is None for record in records[1:])


def test_the_gateway_sender_posts_the_pinned_body_and_reads_to_done() -> None:
    seen: dict[str, object] = {}
    stream = (
        'event: chunk\ndata: {"delta": "hi"}\n\n'
        'event: usage\ndata: {"input_tokens": 5, "provider_used": "openai"}\n\n'
        'event: done\ndata: {"model_used": "gpt-5-mini"}\n\n'
        'event: chunk\ndata: {"delta": "after done"}\n\n'
    )

    def handler(request: httpx.Request) -> httpx.Response:
        seen["path"] = request.url.path
        seen["auth"] = request.headers["Authorization"]
        seen["body"] = json.loads(request.content)
        return httpx.Response(200, content=stream.encode("utf-8"), headers={"content-type": "text/event-stream"})

    body = plan_requests("openai", None, "run-a")[0].body
    with httpx.Client(transport=httpx.MockTransport(handler)) as http:
        events = gateway_sender(http, "http://gateway.test/", "t-9")(body)
    assert seen["path"] == "/v1/invoke/stream"
    assert seen["auth"] == "Bearer t-9"
    assert seen["body"] == body
    assert [event.event for event in events] == ["chunk", "usage", "done"]


def test_a_refused_gateway_request_is_recorded_as_a_failed_turn_without_the_token() -> None:
    transport = httpx.MockTransport(lambda request: httpx.Response(403, json={"error": "forbidden"}))
    with httpx.Client(transport=transport) as http:
        records = run_requests(
            plan_requests("openai", None, "run-a")[:1], gateway_sender(http, "http://gateway.test", "t-9")
        )
    assert records[0].error is not None and records[0].error.startswith("HTTPStatusError")
    assert "t-9" not in records[0].error and "Bearer" not in records[0].error


def test_a_turn_served_by_another_provider_is_counted_off_the_pin() -> None:
    planned = plan_requests("openai", None, "run-a")
    off_pin = record_turn(planned[0], _turn_events(provider="anthropic"))
    on_pin = record_turn(planned[1], _turn_events(provider="openai"))
    assert not off_pin.pinned_route and on_pin.pinned_route
    summary = summarize_turns({"turns": [asdict(off_pin), asdict(on_pin)]})
    assert (summary.sent, summary.failed, summary.off_pinned_route) == (2, 0, 1)


def test_a_failed_turn_is_not_also_counted_as_off_the_pin() -> None:
    def send(body: dict[str, object]) -> list[StreamEvent]:
        raise httpx.ReadTimeout("slow")

    failed = run_requests(plan_requests("openai", None, "run-a")[:1], send)
    summary = summarize_turns({"turns": [asdict(record) for record in failed]})
    assert (summary.failed, summary.off_pinned_route) == (1, 0)


def test_a_turns_file_without_the_recorded_fields_is_refused() -> None:
    with pytest.raises(ValueError, match="turns.json holds a turn"):
        summarize_turns({"turns": [{"kind": "thread"}]})


# ── drive and capture ────────────────────────────────────────────────────


def test_drive_writes_counts_and_a_window_and_never_the_text(tmp_path: Path) -> None:
    arm_dir = tmp_path / "off"
    drive_arm(
        arm_dir,
        arm="off",
        provider="openai",
        model=None,
        send=lambda body: _turn_events("a sentence that must not be stored"),
        settle_s=0,
        run_id="run-a",
    )
    turns = json.loads((arm_dir / TURNS_FILE).read_text(encoding="utf-8"))
    window = json.loads((arm_dir / WINDOW_FILE).read_text(encoding="utf-8"))
    assert turns["arm"] == "off" and turns["run_id"] == "run-a"
    assert len(turns["turns"]) == TOTAL_REQUESTS
    assert all(record["error"] is None for record in turns["turns"])
    assert "must not be stored" not in (arm_dir / TURNS_FILE).read_text(encoding="utf-8")
    assert window["arm"] == "off" and window["since"].endswith("Z")
    assert window["since"] <= window["until"]


def test_drive_refuses_to_overwrite_an_arm_that_was_already_driven(tmp_path: Path) -> None:
    arm_dir = tmp_path / "off"
    drive_arm(arm_dir, arm="off", provider="openai", model=None, send=lambda body: _turn_events(), settle_s=0, run_id="run-a")
    with pytest.raises(FileExistsError):
        drive_arm(arm_dir, arm="off", provider="openai", model=None, send=lambda body: _turn_events(), settle_s=0, run_id="run-b")


def test_capture_saves_the_analytics_for_the_window_it_was_given(tmp_path: Path) -> None:
    arm_dir = tmp_path / "off"
    arm_dir.mkdir()
    (arm_dir / WINDOW_FILE).write_text(
        json.dumps({"arm": "off", "since": "2026-10-24T09:00:00Z", "until": "2026-10-24T10:00:00Z"}),
        encoding="utf-8",
    )
    seen: dict[str, object] = {}

    def handler(request: httpx.Request) -> httpx.Response:
        seen["path"] = request.url.path
        seen["query"] = dict(request.url.params)
        seen["auth"] = request.headers["Authorization"]
        return httpx.Response(200, json=_analytics([_bucket("openai", requests=3, input_tokens=300)]))

    with httpx.Client(transport=httpx.MockTransport(handler)) as http:
        path = capture_analytics(arm_dir, cost_core_url="http://cost-core.test/", token="t-123", http=http)
    assert seen["path"] == ANALYTICS_PATH
    assert seen["query"] == {
        "scope": "organization",
        "since": "2026-10-24T09:00:00Z",
        "until": "2026-10-24T10:00:00Z",
    }
    assert seen["auth"] == "Bearer t-123"
    assert path == arm_dir / ANALYTICS_FILE
    assert json.loads(path.read_text(encoding="utf-8"))["data"]["buckets"][0]["requests"] == 3


def test_capture_writes_nothing_for_a_body_that_is_not_an_analytics_report(tmp_path: Path) -> None:
    arm_dir = tmp_path / "off"
    arm_dir.mkdir()
    (arm_dir / WINDOW_FILE).write_text(
        json.dumps({"arm": "off", "since": "2026-10-24T09:00:00Z", "until": "2026-10-24T10:00:00Z"}),
        encoding="utf-8",
    )
    transport = httpx.MockTransport(lambda request: httpx.Response(200, json={"error": "nope"}))
    with httpx.Client(transport=transport) as http, pytest.raises(ValueError, match="not a cost-core analytics report"):
        capture_analytics(arm_dir, cost_core_url="http://cost-core.test", token="t", http=http)
    assert not (arm_dir / ANALYTICS_FILE).exists()


def test_capture_surfaces_a_refused_request_as_an_http_error(tmp_path: Path) -> None:
    arm_dir = tmp_path / "off"
    arm_dir.mkdir()
    (arm_dir / WINDOW_FILE).write_text(
        json.dumps({"arm": "off", "since": "2026-10-24T09:00:00Z", "until": "2026-10-24T10:00:00Z"}),
        encoding="utf-8",
    )
    transport = httpx.MockTransport(lambda request: httpx.Response(403, json={"error": "no scope"}))
    with httpx.Client(transport=transport) as http, pytest.raises(httpx.HTTPStatusError):
        capture_analytics(arm_dir, cost_core_url="http://cost-core.test", token="t", http=http)


# ── analytics and eval reports ───────────────────────────────────────────


def test_analytics_sums_only_the_pinned_providers() -> None:
    summary = summarize_analytics(
        _analytics(
            [
                _bucket("openai", requests=5, input_tokens=1000, read=600, reported=1000),
                _bucket("azure-openai", requests=2, input_tokens=500, read=100, creation=50, reported=500),
                _bucket("anthropic", requests=9, input_tokens=4000, read=3000, reported=4000),
            ],
            misses=[
                {"day": "2026-10-24", "model": "m", "provider": "openai", "reason": "ttl_expired", "requests": 1},
                {"day": "2026-10-24", "model": "m", "provider": "anthropic", "reason": "prefix_changed", "requests": 4},
            ],
        )
    )
    assert summary.requests == 7
    assert summary.input_tokens == 1500 and summary.all_input_tokens == 5500
    assert summary.reported_input_tokens == 1500
    assert summary.read_tokens == 700 and summary.creation_tokens == 50
    assert summary.hit_rate == pytest.approx(700 / 1500)
    assert summary.write_share == pytest.approx(50 / 1500)
    assert summary.pinned_share == pytest.approx(1500 / 5500)
    assert summary.miss_reasons == {"ttl_expired": 1}


def test_an_arm_with_no_reported_cache_activity_has_no_hit_rate() -> None:
    summary = summarize_analytics(_analytics([_bucket("openai", requests=3, input_tokens=900)]))
    assert summary.hit_rate is None and summary.write_share is None
    comparison = render_comparison(_arm("off", _analytics([])), _arm("on", _analytics([_bucket("openai", requests=3, input_tokens=900)])))
    assert "No cache activity reported" in comparison


def test_analytics_refuses_a_payload_that_is_not_a_cost_report() -> None:
    with pytest.raises(ValueError, match="not a cost-core analytics report"):
        summarize_analytics({"error": "forbidden"})
    with pytest.raises(ValueError, match="not a cost-core analytics report"):
        summarize_analytics({"data": {"buckets": [{"provider": "openai"}], "cache_breakdown": {"miss_reasons": []}}})


def test_an_eval_report_is_read_back_as_counts_and_verdicts() -> None:
    results = [
        CaseResult(
            case_id="01-ok",
            passed=True,
            metrics=[MetricOutcome(metric="accuracy", passed=True, score=1.0)],
        ),
        CaseResult(
            case_id="02-bad",
            passed=False,
            metrics=[MetricOutcome(metric="cost", passed=False, score=0.0, detail="over ceiling")],
        ),
        CaseResult(case_id="03-later", passed=False, skipped=True, skip_reason="no fixture"),
    ]
    report = parse_eval_report(render_report(results))
    assert (report.passed, report.ran) == (1, 2)
    assert report.verdicts == {"01-ok": "PASS", "02-bad": "FAIL", "03-later": "SKIP"}
    assert report.pass_rate == pytest.approx(0.5)


def test_an_eval_report_parser_refuses_text_without_a_summary_line() -> None:
    with pytest.raises(ValueError, match="not an eval report"):
        parse_eval_report("# Eval report\n\n| Case | Result |\n|---|---|\n")


def test_parity_names_a_case_whose_verdicts_differ_between_the_arms() -> None:
    off = [EvalReport(2, 2, {"ok": "PASS", "x": "PASS"}), EvalReport(2, 2, {"ok": "PASS", "x": "PASS"})]
    on = [EvalReport(2, 2, {"ok": "PASS", "x": "PASS"}), EvalReport(1, 2, {"ok": "FAIL", "x": "PASS"})]
    assert parity_differences(off, on) == ["ok: off {PASS}, on {FAIL, PASS}"]


def test_parity_is_empty_when_the_arms_agree_and_names_a_case_missing_from_one_arm() -> None:
    assert parity_differences([EvalReport(1, 1, {"a": "PASS"})], [EvalReport(1, 1, {"a": "PASS"})]) == []
    assert parity_differences([EvalReport(1, 1, {"a": "PASS"})], []) == ["a: off {PASS}, on missing"]


def test_compare_reads_the_layout_the_runbook_names(tmp_path: Path) -> None:
    root = tmp_path / "ab"
    _write_arm(
        root / "off",
        _analytics([_bucket("openai", requests=10, input_tokens=1000, read=600, reported=1000)]),
        {"rerun-1": _half_passing_report(), "rerun-2": _half_passing_report()},
    )
    _write_arm(
        root / "on",
        _analytics([_bucket("openai", requests=10, input_tokens=1000, read=900, reported=1000)]),
        {"rerun-1": _half_passing_report(), "rerun-2": _half_passing_report()},
    )
    text = compare_evidence(root)
    assert "| off | 10 |" in text and "| on | 10 |" in text
    assert "hit rate +30.0 pp" in text
    assert "eval pass rate +0.0 pp" in text
    assert "Eval parity (cases whose verdicts differ between the arms):\n- none" in text


def test_load_arm_reads_no_eval_reports_when_none_were_run(tmp_path: Path) -> None:
    _write_arm(tmp_path / "off", _analytics([_bucket("openai", requests=1, input_tokens=10)]), {})
    evidence = load_arm(tmp_path / "off", "off")
    assert evidence.evals == []
    assert "| off | 1 |" in render_comparison(evidence, _arm("on", _analytics([])))


# ── routes, plan and the command line ────────────────────────────────────


def test_the_plan_routes_appear_in_the_source_that_registers_them() -> None:
    root = default_repo_root()
    if root is None:
        pytest.skip("the repository is not present around this package")
    checks = route_check(root)
    assert checks and all(check["route_found"] for check in checks.values())


def test_a_route_check_without_a_repository_reports_the_sources_as_absent() -> None:
    checks = route_check(None)
    assert checks and not any(check["source_present"] or check["route_found"] for check in checks.values())


def test_plan_prints_every_request_and_sends_nothing(capsys: pytest.CaptureFixture[str]) -> None:
    root = default_repo_root()
    if root is None:
        pytest.skip("the repository is not present around this package")
    assert main(["plan", "--provider", "openai", "--repo-root", str(root)]) == 0
    output = capsys.readouterr().out
    report = json.loads(output)
    assert len(report["requests"]) == TOTAL_REQUESTS
    assert {request["body"]["provider"] for request in report["requests"]} == {"openai"}
    assert "Bearer" not in output


def test_drive_refuses_to_run_without_the_internal_key(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, capsys: pytest.CaptureFixture[str]
) -> None:
    monkeypatch.delenv("INTERNAL_API_KEY", raising=False)
    assert main(["drive", "--arm", "off", "--provider", "openai", "--evidence-root", str(tmp_path)]) == 2
    assert "INTERNAL_API_KEY is not set" in capsys.readouterr().err
    assert not (tmp_path / "off").exists()


def test_capture_needs_its_token_from_the_environment(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, capsys: pytest.CaptureFixture[str]
) -> None:
    monkeypatch.delenv("EVAL_COST_TOKEN", raising=False)
    code = main(
        ["capture", "--arm", "off", "--evidence-root", str(tmp_path), "--cost-core-url", "http://cost-core.test"]
    )
    assert code == 2
    assert "EVAL_COST_TOKEN" in capsys.readouterr().err


def test_compare_on_an_empty_evidence_root_exits_with_an_error(
    tmp_path: Path, capsys: pytest.CaptureFixture[str]
) -> None:
    assert main(["compare", "--evidence-root", str(tmp_path / "missing")]) == 2
    assert "compare:" in capsys.readouterr().err
