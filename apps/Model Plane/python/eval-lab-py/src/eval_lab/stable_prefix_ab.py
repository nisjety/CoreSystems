"""Staging A/B for the stable-prefix prompt order (WS02-S8).

`MODEL_GATEWAY_CACHE_STABLE_PROMPT_ORDER` changes the prompt only on a turn
pinned to an OpenAI-API chat-completions provider (`CacheRoute` in
prompt_context.rs). Every request this module sends therefore pins `provider`,
and the comparison reports how much of the traffic the pin actually served.

Subcommands, one arm at a time (`off`, then `on`), under one evidence root:

- plan: prints the requests a run would send and checks that the routes they
  use appear in the source tree. It sends nothing.
- drive: sends three threads of eight turns and two Do runs, then writes
  turns.json (counts and identifiers, never text) and window.json.
- capture: saves the cost-core analytics for the arm's window.
- compare: prints the A/B table from the captures and the eval reruns.

The operator sets the flag on model-gateway and restarts it between the arms,
because the flag is read once per process. This module never sets it.
"""

from __future__ import annotations

import argparse
import json
import os
import re
import sys
import time
import uuid
from collections.abc import Callable, Sequence
from dataclasses import asdict, dataclass
from datetime import datetime, timezone
from pathlib import Path
from statistics import mean

import httpx

from eval_lab.client import EvalEnv, VerevonClient, parse_sse_lines
from eval_lab.runner import FIXTURE_ORGS
from eval_lab.types import StreamEvent

ARMS = ("off", "on")
# The providers `CacheRoute::of_turn` resolves to an OpenAI-API chat-completions
# route. The flag changes nothing on any other provider.
PINNED_PROVIDERS = ("openai", "azure-openai")
THREADS = 3
DO_RUNS = 2
THREAD_TURNS: tuple[str, ...] = (
    "I am preparing a food-safety audit for a dairy plant. Name the three areas to check first.",
    "Take the second of those areas and list the findings auditors most often write up there.",
    "Which of those findings is missed most often, and why?",
    "Draft a five-item checklist for the first area.",
    "Shorten that checklist to three items without dropping the critical one.",
    "What evidence should the auditor ask to see for the first area?",
    "Summarise the plan so far in four bullet points.",
    "What should I verify before the audit starts? Answer in two sentences.",
)
DO_GOAL = (
    "List the three steps you would take to prepare an audit folder for a dairy plant. "
    "Do not call any tool."
)
ORG_FIXTURE = "eval-org-a"
DEFAULT_SETTLE_S = 20.0
TURN_TIMEOUT_S = 300.0
TERMINAL_EVENTS = ("done", "error", "stopped")
GATEWAY_INVOKE_PATH = "/v1/invoke/stream"
ANALYTICS_PATH = "/api/v1/cost/analytics"
ROUTE_SOURCES = {
    GATEWAY_INVOKE_PATH: "apps/Model Plane/rust/services/model-gateway/src/http_routes.rs",
    ANALYTICS_PATH: "apps/Model Plane/go/services/cost-core/internal/server/server.go",
}
WINDOW_FILE = "window.json"
TURNS_FILE = "turns.json"
ANALYTICS_FILE = "analytics.json"

Sender = Callable[[dict[str, object]], list[StreamEvent]]


# ── the traffic ──────────────────────────────────────────────────────────


@dataclass(frozen=True)
class PlannedRequest:
    kind: str  # "thread" or "do"
    group: int  # the thread or the Do run, counted from 1
    turn: int  # the turn within a thread; 1 for a Do run
    body: dict[str, object]


def plan_requests(provider: str, model: str | None, run_id: str) -> list[PlannedRequest]:
    if provider not in PINNED_PROVIDERS:
        raise ValueError(f"provider must be one of {PINNED_PROVIDERS}, got {provider!r}")
    planned: list[PlannedRequest] = []
    for thread in range(1, THREADS + 1):
        session_key = f"stable-prefix:{run_id}:thread-{thread}"
        for turn, content in enumerate(THREAD_TURNS, start=1):
            body = _body(
                content,
                session_key,
                f"{session_key}:turn-{turn}",
                provider,
                model,
                profile="chat",
                features=["usage"],
            )
            planned.append(PlannedRequest("thread", thread, turn, body))
    for run in range(1, DO_RUNS + 1):
        session_key = f"stable-prefix:{run_id}:do-{run}"
        body = _body(
            DO_GOAL,
            session_key,
            f"{session_key}:goal",
            provider,
            model,
            profile="deployed_agent",
            features=["agentic", "usage"],
        )
        planned.append(PlannedRequest("do", run, 1, body))
    return planned


def _body(
    content: str,
    session_key: str,
    idempotency_key: str,
    provider: str,
    model: str | None,
    *,
    profile: str,
    features: list[str],
) -> dict[str, object]:
    body: dict[str, object] = {
        "content": content,
        "profile": profile,
        "features": features,
        "zdr": False,
        "provider": provider,
        "session_key": session_key,
        "idempotency_key": idempotency_key,
    }
    if model:
        body["model"] = model
    return body


def gateway_sender(http: httpx.Client, gateway_url: str, token: str) -> Sender:
    """One POST to /v1/invoke/stream, read to its terminal event."""

    def send(body: dict[str, object]) -> list[StreamEvent]:
        events: list[StreamEvent] = []
        with http.stream(
            "POST",
            f"{gateway_url.rstrip('/')}{GATEWAY_INVOKE_PATH}",
            headers={"Authorization": f"Bearer {token}", "Accept": "text/event-stream"},
            json=body,
            timeout=httpx.Timeout(TURN_TIMEOUT_S, connect=10.0),
        ) as response:
            response.raise_for_status()
            for event in parse_sse_lines(response.iter_lines()):
                events.append(event)
                if event.event in TERMINAL_EVENTS:
                    break
        return events

    return send


# ── one turn, recorded as counts ─────────────────────────────────────────


@dataclass(frozen=True)
class TurnRecord:
    kind: str
    group: int
    turn: int
    request_id: str | None
    provider_used: str | None
    model_used: str | None
    input_tokens: int | None
    output_tokens: int | None
    cache_read_tokens: int
    cache_write_tokens: int
    text_chars: int
    error: str | None
    pinned_route: bool


def record_turn(request: PlannedRequest, events: Sequence[StreamEvent]) -> TurnRecord:
    request_id: str | None = None
    provider_used: str | None = None
    model_used: str | None = None
    input_tokens: int | None = None
    output_tokens: int | None = None
    cache_read = 0
    cache_write = 0
    text_chars = 0
    error: str | None = None
    terminal = False
    for event in events:
        data = event.data
        request_id = request_id or _text(data.get("request_id"))
        if event.event == "chunk":
            text_chars += len(_text(data.get("delta")) or "")
        elif event.event == "usage":
            provider_used = _text(data.get("provider_used")) or provider_used
            input_tokens = _add(input_tokens, data.get("input_tokens"))
            output_tokens = _add(output_tokens, data.get("output_tokens"))
            cache_read += _count(data.get("cache_read_tokens"))
            cache_write += _count(data.get("cache_write_tokens"))
        elif event.event == "done":
            model_used = _text(data.get("model_used")) or model_used
        elif event.event == "error" and error is None:
            error = _text(data.get("code")) or "error event"
        elif event.event == "stopped" and error is None:
            error = "stopped"
        if event.event in TERMINAL_EVENTS:
            terminal = True
    if error is None and not terminal:
        error = "stream ended before done"
    return TurnRecord(
        kind=request.kind,
        group=request.group,
        turn=request.turn,
        request_id=request_id,
        provider_used=provider_used,
        model_used=model_used,
        input_tokens=input_tokens,
        output_tokens=output_tokens,
        cache_read_tokens=cache_read,
        cache_write_tokens=cache_write,
        text_chars=text_chars,
        error=error,
        pinned_route=provider_used in PINNED_PROVIDERS,
    )


def run_requests(planned: Sequence[PlannedRequest], send: Sender) -> list[TurnRecord]:
    """Every request is sent even after a failure: a failed turn is a finding
    the comparison reports, not a reason to end the arm early."""
    records: list[TurnRecord] = []
    for request in planned:
        try:
            events = send(request.body)
        except httpx.HTTPError as error:
            records.append(_failed(request, f"{type(error).__name__}: {error}"))
            continue
        records.append(record_turn(request, events))
    return records


def _failed(request: PlannedRequest, error: str) -> TurnRecord:
    return TurnRecord(
        kind=request.kind,
        group=request.group,
        turn=request.turn,
        request_id=None,
        provider_used=None,
        model_used=None,
        input_tokens=None,
        output_tokens=None,
        cache_read_tokens=0,
        cache_write_tokens=0,
        text_chars=0,
        error=error,
        pinned_route=False,
    )


def _text(value: object) -> str | None:
    return value if isinstance(value, str) and value else None


def _count(value: object) -> int:
    return value if isinstance(value, int) and not isinstance(value, bool) else 0


def _add(total: int | None, value: object) -> int | None:
    if isinstance(value, int) and not isinstance(value, bool):
        return (total or 0) + value
    return total


# ── drive and capture ────────────────────────────────────────────────────


def utc_timestamp() -> str:
    return datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")


def drive_arm(
    arm_dir: Path,
    *,
    arm: str,
    provider: str,
    model: str | None,
    send: Sender,
    settle_s: float,
    run_id: str | None = None,
) -> None:
    run = run_id or uuid.uuid4().hex[:12]
    planned = plan_requests(provider, model, run)
    if (arm_dir / WINDOW_FILE).exists():
        raise FileExistsError(f"{arm_dir} already holds a drive; use a new evidence root")
    arm_dir.mkdir(parents=True, exist_ok=True)
    since = utc_timestamp()
    records = run_requests(planned, send)
    # The ledger row of a turn is written after its stream ends, so the window
    # closes after a settle period, not at the last byte.
    time.sleep(settle_s)
    until = utc_timestamp()
    _write_json(
        arm_dir / TURNS_FILE,
        {
            "arm": arm,
            "provider": provider,
            "model": model,
            "run_id": run,
            "turns": [asdict(record) for record in records],
        },
    )
    _write_json(arm_dir / WINDOW_FILE, {"arm": arm, "since": since, "until": until})


def capture_analytics(
    arm_dir: Path, *, cost_core_url: str, token: str, http: httpx.Client
) -> Path:
    window = _read_json(arm_dir / WINDOW_FILE)
    response = http.get(
        f"{cost_core_url.rstrip('/')}{ANALYTICS_PATH}",
        params={"scope": "organization", "since": window["since"], "until": window["until"]},
        headers={"Authorization": f"Bearer {token}"},
    )
    response.raise_for_status()
    payload = response.json()
    # Refuses a body that is not an analytics report before anything is written.
    summarize_analytics(payload)
    return _write_json(arm_dir / ANALYTICS_FILE, payload)


def _write_json(path: Path, payload: object) -> Path:
    path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    return path


def _read_json(path: Path) -> dict[str, object]:
    payload = json.loads(path.read_text(encoding="utf-8"))
    if not isinstance(payload, dict):
        raise ValueError(f"{path} does not hold a JSON object")
    return payload


# ── compare ──────────────────────────────────────────────────────────────


@dataclass(frozen=True)
class CacheSummary:
    requests: int
    input_tokens: int
    all_input_tokens: int
    reported_input_tokens: int
    read_tokens: int
    creation_tokens: int
    miss_reasons: dict[str, int]

    @property
    def hit_rate(self) -> float | None:
        return _ratio(self.read_tokens, self.reported_input_tokens)

    @property
    def write_share(self) -> float | None:
        return _ratio(self.creation_tokens, self.reported_input_tokens)

    @property
    def pinned_share(self) -> float | None:
        return _ratio(self.input_tokens, self.all_input_tokens)


def summarize_analytics(payload: dict[str, object]) -> CacheSummary:
    """Sums the buckets of the pinned providers. Hit rate is read over the input
    the provider reported cache activity for, as the ledger defines it."""
    try:
        data = payload["data"]
        requests = input_tokens = all_input = reported = read = creation = 0
        for bucket in data["buckets"]:
            tokens = int(bucket["input_tokens"])
            all_input += tokens
            if str(bucket["provider"]) in PINNED_PROVIDERS:
                requests += int(bucket["requests"])
                input_tokens += tokens
                reported += int(bucket["cache_reported_input_tokens"])
                read += int(bucket["cache_read_input_tokens"])
                creation += int(bucket["cache_creation_input_tokens"])
        reasons: dict[str, int] = {}
        for miss in data["cache_breakdown"]["miss_reasons"]:
            if str(miss["provider"]) in PINNED_PROVIDERS:
                reason = str(miss["reason"])
                reasons[reason] = reasons.get(reason, 0) + int(miss["requests"])
    except (KeyError, TypeError, ValueError) as error:
        raise ValueError(
            "not a cost-core analytics report (needs data.buckets and data.cache_breakdown)"
        ) from error
    return CacheSummary(
        requests=requests,
        input_tokens=input_tokens,
        all_input_tokens=all_input,
        reported_input_tokens=reported,
        read_tokens=read,
        creation_tokens=creation,
        miss_reasons=reasons,
    )


@dataclass(frozen=True)
class TurnSummary:
    sent: int
    failed: int
    off_pinned_route: int
    do_without_error: int


def summarize_turns(payload: dict[str, object]) -> TurnSummary:
    turns = payload.get("turns")
    if not isinstance(turns, list):
        raise ValueError("turns.json has no turns list")
    try:
        succeeded = [turn for turn in turns if not turn["error"]]
        return TurnSummary(
            sent=len(turns),
            failed=len(turns) - len(succeeded),
            off_pinned_route=sum(1 for turn in succeeded if not turn["pinned_route"]),
            # A Do run that paused for approval still ends in done, so this counts
            # runs without an error, not runs that finished their work.
            do_without_error=sum(1 for turn in succeeded if turn["kind"] == "do"),
        )
    except (KeyError, TypeError) as error:
        raise ValueError("turns.json holds a turn without its recorded fields") from error


_EVAL_SUMMARY = re.compile(r"\*\*(\d+)/(\d+) passed\*\*")
_EVAL_ROW = re.compile(r"^\|\s*([^|]+?)\s*\|\s*([^|]+?)\s*\|")


@dataclass(frozen=True)
class EvalReport:
    passed: int
    ran: int
    verdicts: dict[str, str]

    @property
    def pass_rate(self) -> float | None:
        return _ratio(self.passed, self.ran)


def parse_eval_report(text: str) -> EvalReport:
    """Reads a report written by eval_lab.report.render_report."""
    summary = _EVAL_SUMMARY.search(text)
    if summary is None:
        raise ValueError("not an eval report: no '**passed/ran passed**' summary line")
    verdicts: dict[str, str] = {}
    for line in text.splitlines():
        row = _EVAL_ROW.match(line)
        if row is None:
            continue
        label = next((name for name in ("PASS", "FAIL", "SKIP") if name in row.group(2)), None)
        if label is not None:
            verdicts[row.group(1)] = label
    return EvalReport(int(summary.group(1)), int(summary.group(2)), verdicts)


def parity_differences(off: Sequence[EvalReport], on: Sequence[EvalReport]) -> list[str]:
    """Cases whose set of verdicts, collected over an arm's reruns, differs
    between the arms. A flip that both arms share does not appear here; the
    pass-rate range in the table shows the flips inside each arm."""
    left = _labels_by_case(off)
    right = _labels_by_case(on)
    lines: list[str] = []
    for case in sorted(set(left) | set(right)):
        before = left.get(case, set())
        after = right.get(case, set())
        if before != after:
            lines.append(f"{case}: off {_show(before)}, on {_show(after)}")
    return lines


def _labels_by_case(reports: Sequence[EvalReport]) -> dict[str, set[str]]:
    labels: dict[str, set[str]] = {}
    for report in reports:
        for case, verdict in report.verdicts.items():
            labels.setdefault(case, set()).add(verdict)
    return labels


def _show(labels: set[str]) -> str:
    return "{" + ", ".join(sorted(labels)) + "}" if labels else "missing"


@dataclass(frozen=True)
class ArmEvidence:
    arm: str
    cache: CacheSummary
    turns: TurnSummary
    evals: list[EvalReport]


def load_arm(arm_dir: Path, arm: str) -> ArmEvidence:
    return ArmEvidence(
        arm=arm,
        cache=summarize_analytics(_read_json(arm_dir / ANALYTICS_FILE)),
        turns=summarize_turns(_read_json(arm_dir / TURNS_FILE)),
        evals=[
            parse_eval_report(path.read_text(encoding="utf-8"))
            for path in sorted(arm_dir.glob("eval/*/*.md"))
        ],
    )


def compare_evidence(evidence_root: Path) -> str:
    off = load_arm(evidence_root / "off", "off")
    on = load_arm(evidence_root / "on", "on")
    return render_comparison(off, on)


def render_comparison(off: ArmEvidence, on: ArmEvidence) -> str:
    lines = [
        "| Arm | Requests on pinned route | Share of all input on pinned route "
        "| Input tokens on pinned route | Cache-reported input | Cache read | Hit rate "
        "| Cache write share | Eval reruns | Eval pass rate |",
        "|---|---|---|---|---|---|---|---|---|---|",
        _arm_row(off),
        _arm_row(on),
        "",
        f"Delta, on minus off: hit rate {_pp_delta(off.cache.hit_rate, on.cache.hit_rate)}; "
        f"eval pass rate {_pp_delta(_mean_rate(off.evals), _mean_rate(on.evals))}.",
        "",
        "Drive, per arm (requests sent, failed, served off the pinned route, Do runs without error):",
        f"- off: {_drive_line(off.turns)}",
        f"- on: {_drive_line(on.turns)}",
        "",
        "Miss reasons on the pinned route:",
        f"- off: {_reasons(off.cache.miss_reasons)}",
        f"- on: {_reasons(on.cache.miss_reasons)}",
        "",
        "Eval parity (cases whose verdicts differ between the arms):",
    ]
    differences = parity_differences(off.evals, on.evals)
    if differences:
        lines.extend(f"- {line}" for line in differences)
    else:
        lines.append("- none")
    return "\n".join(lines) + "\n"


def _arm_row(evidence: ArmEvidence) -> str:
    cache = evidence.cache
    return (
        f"| {evidence.arm} | {cache.requests} | {_percent(cache.pinned_share)} "
        f"| {cache.input_tokens} | {cache.reported_input_tokens} | {cache.read_tokens} "
        f"| {_rate(cache.hit_rate)} | {_rate(cache.write_share)} "
        f"| {len(evidence.evals)} | {_eval_cell(evidence.evals)} |"
    )


def _rate(value: float | None) -> str:
    return "No cache activity reported" if value is None else f"{value * 100:.1f}%"


def _percent(value: float | None) -> str:
    return "n/a" if value is None else f"{value * 100:.1f}%"


def _rates(reports: Sequence[EvalReport]) -> list[float]:
    return [rate for report in reports if (rate := report.pass_rate) is not None]


def _mean_rate(reports: Sequence[EvalReport]) -> float | None:
    rates = _rates(reports)
    return mean(rates) if rates else None


def _eval_cell(reports: Sequence[EvalReport]) -> str:
    rates = _rates(reports)
    if not rates:
        return "no reports"
    return (
        f"{mean(rates) * 100:.1f}% (min {min(rates) * 100:.1f}%, "
        f"max {max(rates) * 100:.1f}%)"
    )


def _pp_delta(before: float | None, after: float | None) -> str:
    if before is None or after is None:
        return "n/a"
    return f"{(after - before) * 100:+.1f} pp"


def _drive_line(turns: TurnSummary) -> str:
    return (
        f"{turns.sent} sent, {turns.failed} failed, {turns.off_pinned_route} off the "
        f"pinned route, {turns.do_without_error} of {DO_RUNS} Do runs without error"
    )


def _reasons(reasons: dict[str, int]) -> str:
    if not reasons:
        return "none"
    ordered = sorted(reasons.items(), key=lambda item: (-item[1], item[0]))
    return ", ".join(f"{name} {count}" for name, count in ordered)


def _ratio(part: int, whole: int) -> float | None:
    return part / whole if whole else None


# ── routes and the command line ──────────────────────────────────────────


def default_repo_root() -> Path | None:
    for ancestor in Path(__file__).resolve().parents:
        if (ancestor / "apps" / "Model Plane").is_dir():
            return ancestor
    return None


def route_check(repo_root: Path | None) -> dict[str, dict[str, object]]:
    """A substring check that each route path appears in the file that
    registers it. It is a dry-run aid, not a route parser."""
    checks: dict[str, dict[str, object]] = {}
    for route, source in ROUTE_SOURCES.items():
        path = repo_root / source if repo_root is not None else None
        present = path is not None and path.is_file()
        found = present and route in path.read_text(encoding="utf-8")
        checks[route] = {"source": source, "source_present": present, "route_found": found}
    return checks


def main(argv: Sequence[str] | None = None) -> int:
    args = _parser().parse_args(argv)
    handlers = {"plan": _plan, "drive": _drive, "capture": _capture, "compare": _compare}
    try:
        return handlers[args.command](args)
    except (OSError, ValueError) as error:
        print(f"{args.command}: {error}", file=sys.stderr)
        return 2


def _plan(args: argparse.Namespace) -> int:
    routes = route_check(args.repo_root or default_repo_root())
    planned = plan_requests(args.provider, args.model, run_id="dry-run")
    report = {
        "provider": args.provider,
        "model": args.model,
        "routes": routes,
        "requests": [
            {"kind": request.kind, "group": request.group, "turn": request.turn, "body": request.body}
            for request in planned
        ],
    }
    print(json.dumps(report, indent=2, sort_keys=True))
    missing = [
        route
        for route, check in routes.items()
        if check["source_present"] and not check["route_found"]
    ]
    return 1 if missing else 0


def _drive(args: argparse.Namespace) -> int:
    env = EvalEnv()
    ready, reason = env.ready()
    if not ready:
        print(f"drive: {reason}", file=sys.stderr)
        return 2
    arm_dir = args.evidence_root / args.arm
    client = VerevonClient(env)
    try:
        token = client.mint_token(*FIXTURE_ORGS[ORG_FIXTURE])
        with httpx.Client() as http:
            drive_arm(
                arm_dir,
                arm=args.arm,
                provider=args.provider,
                model=args.model,
                send=gateway_sender(http, env.model_gateway_url, token),
                settle_s=args.settle_seconds,
            )
    except (httpx.HTTPError, RuntimeError) as error:
        print(f"drive: token or gateway setup failed ({type(error).__name__})", file=sys.stderr)
        return 1
    finally:
        client.close()
    print(f"drive: wrote {arm_dir}")
    return 0


def _capture(args: argparse.Namespace) -> int:
    token = os.environ.get(args.token_env, "")
    if not token:
        print(
            f"capture: set {args.token_env} to a cost:org:read token for the eval organisation",
            file=sys.stderr,
        )
        return 2
    try:
        with httpx.Client(timeout=60.0) as http:
            path = capture_analytics(
                args.evidence_root / args.arm,
                cost_core_url=args.cost_core_url,
                token=token,
                http=http,
            )
    except httpx.HTTPError as error:
        status = error.response.status_code if isinstance(error, httpx.HTTPStatusError) else None
        detail = f"HTTP {status}" if status is not None else type(error).__name__
        print(f"capture: cost-core request failed ({detail})", file=sys.stderr)
        return 1
    print(f"capture: wrote {path}")
    return 0


def _compare(args: argparse.Namespace) -> int:
    sys.stdout.write(compare_evidence(args.evidence_root))
    return 0


def _parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        prog="python -m eval_lab.stable_prefix_ab",
        description="Staging A/B for the stable-prefix prompt order (WS02-S8).",
    )
    commands = parser.add_subparsers(dest="command", required=True)

    plan = commands.add_parser("plan", help="print the planned requests and check their routes")
    plan.add_argument("--provider", required=True, choices=PINNED_PROVIDERS)
    plan.add_argument("--model")
    plan.add_argument("--repo-root", type=Path, help="defaults to the repository holding this module")

    drive = commands.add_parser("drive", help="send the traffic of one arm through model-gateway")
    _arm_arguments(drive)
    drive.add_argument("--provider", required=True, choices=PINNED_PROVIDERS)
    drive.add_argument("--model")
    drive.add_argument("--settle-seconds", type=float, default=DEFAULT_SETTLE_S)

    capture = commands.add_parser("capture", help="save cost-core analytics for the arm's window")
    _arm_arguments(capture)
    capture.add_argument("--cost-core-url", required=True)
    capture.add_argument(
        "--token-env",
        default="EVAL_COST_TOKEN",
        help="name of the environment variable holding the bearer token",
    )

    compare = commands.add_parser("compare", help="print the A/B table for an evidence root")
    compare.add_argument("--evidence-root", required=True, type=Path)
    return parser


def _arm_arguments(parser: argparse.ArgumentParser) -> None:
    parser.add_argument("--arm", required=True, choices=ARMS)
    parser.add_argument("--evidence-root", required=True, type=Path)


if __name__ == "__main__":
    raise SystemExit(main())
