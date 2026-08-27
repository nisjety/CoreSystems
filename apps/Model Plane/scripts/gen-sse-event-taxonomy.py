#!/usr/bin/env python3
"""Generate (or verify) the SSE chat-event taxonomy from source.

Why this exists
---------------
This repo has a documented, recurring stale-docs problem: prose claiming a
smaller SSE surface than the code emits. The endpoint map is *curated* — its
`v3` relevance ratings and "do not wire yet" flags are human judgment, and
regenerating it would destroy them — so the drift gate belongs on the part that
really is mechanical: the chat event taxonomy.

It checks two things a comment cannot:

1.  **Producer -> consumer coverage.** Every `ChatEvent` the gateway can emit
    must have a matching `case '<name>':` in the SPA's stream client. An event
    added on the backend with no client case is silently dropped: the stream
    succeeds, the feature is invisible, and nothing fails. (The client's
    `onUnknownEvent` hook makes it *loggable* at runtime; this makes it fail at
    build time, which is where it is cheap.)
2.  **Doc freshness.** The committed taxonomy doc must equal what source says.

Usage:
    gen-sse-event-taxonomy.py            # rewrite the doc
    gen-sse-event-taxonomy.py --check    # fail on drift, print a legible diff
"""

from __future__ import annotations

import argparse
import difflib
import re
import sys
from pathlib import Path

# apps/Model Plane/scripts/ -> repo root
REPO_ROOT = Path(__file__).resolve().parents[3]
PRODUCER = REPO_ROOT / "apps/Model Plane/rust/services/model-gateway/src/sse_events.rs"
CONSUMER = REPO_ROOT / "apps/Frontend Plane/verevonv3/src/shared/api/chat-client.ts"
DOC = REPO_ROOT / "apps/Model Plane/docs/sse-event-taxonomy.md"

# Names the SPA handles that the ChatEvent enum does not define, with the reason.
# These are asserted to STILL be handled, so deleting one is also caught.
NON_CHATEVENT_CONSUMER_CASES = {
    "connected": "transport: stream opened, emitted by the SSE layer",
    "chunk": "transport: assistant text delta",
    "done": "transport: terminal success frame",
    "citations": "legacy alias retained for older gateway builds",
    "search_results": "legacy alias retained for older gateway builds",
}


def read(path: Path) -> str:
    try:
        return path.read_text()
    except OSError as error:
        sys.exit(
            f"cannot read {path} ({error}).\n"
            "If the file moved, re-point this script — do not delete the check; "
            "it is the only thing that fails when the backend and the SPA "
            "disagree about which events exist."
        )


def fn_body(source: str, signature: str) -> str:
    """Return the text of a function body, brace-matched from its signature."""
    start = source.find(signature)
    if start < 0:
        sys.exit(f"`{signature}` not found in {PRODUCER.name} — was it renamed?")
    open_brace = source.index("{", start)
    depth = 0
    for index in range(open_brace, len(source)):
        if source[index] == "{":
            depth += 1
        elif source[index] == "}":
            depth -= 1
            if depth == 0:
                return source[open_brace + 1 : index]
    sys.exit(f"unbalanced braces after `{signature}`")


VARIANT_ARM = re.compile(
    r"((?:ChatEvent::\w+\s*\{\s*\.\.\s*\}\s*\|?\s*)+)=>\s*(?:Some\(\"(\w+)\"\)|None|\"(\w+)\")"
)


def strip_line_comments(body: str) -> str:
    """Remove `//` comments so a comment cannot break a match-arm pattern list.

    This is not cosmetic. `VARIANT_ARM` requires an unbroken chain of
    `ChatEvent::X { .. } |` patterns up to the `=>`, and `family()` documents its
    control-event group with a comment sitting BETWEEN two of those patterns.
    Flattening whitespace left the comment text inline, the chain broke, and the
    match began after it — so `Title` and `FollowUps` were reported as having no
    family arm when both are plainly in the `=> None` group. The gate had been
    failing on that false positive, which is how a red CI gate stops being read.

    Only whole-line and trailing comments are stripped, and only outside string
    literals — the arms here carry `Some("name")` values that must survive.
    """
    out: list[str] = []
    for line in body.split("\n"):
        in_string = False
        cut = None
        index = 0
        while index < len(line) - 1:
            char = line[index]
            if char == '"' and (index == 0 or line[index - 1] != "\\"):
                in_string = not in_string
            elif not in_string and char == "/" and line[index + 1] == "/":
                cut = index
                break
            index += 1
        out.append(line if cut is None else line[:cut])
    return "\n".join(out)


def parse_arms(body: str) -> dict[str, str | None]:
    """Map each ChatEvent variant to its arm value (None for a `None` arm)."""
    flat = " ".join(strip_line_comments(body).split())
    out: dict[str, str | None] = {}
    for match in VARIANT_ARM.finditer(flat):
        value = match.group(2) or match.group(3)
        for variant in re.findall(r"ChatEvent::(\w+)", match.group(1)):
            out[variant] = value
    if not out:
        sys.exit("parsed zero match arms — the parser broke, not the invariant")
    return out


def consumer_cases(source: str) -> set[str]:
    cases = set(re.findall(r"case '([a-z_]+)':", source))
    if not cases:
        sys.exit(f"parsed zero `case '...'` labels from {CONSUMER.name}")
    return cases


def render(events: list[tuple[str, str, str | None]]) -> str:
    lines = [
        "# SSE chat-event taxonomy",
        "",
        "<!-- GENERATED FILE — do not edit by hand.",
        "     Regenerate: apps/Model Plane/scripts/gen-sse-event-taxonomy.py",
        "     Verified in CI by .github/workflows/sse-taxonomy.yml -->",
        "",
        "The events `model-gateway` can emit on a chat stream, derived from the",
        "`ChatEvent` enum in `model-gateway/src/sse_events.rs`.",
        "",
        "`Family` is the opt-in feature family: a client receives a rich event only",
        "if it listed that family in `features[]`. Events with **no** family are",
        "control events and always emit — that is what keeps the plain `chat`",
        "profile working when a client asks for no features at all.",
        "",
        "`SPA` is whether `verevonv3`'s stream client has a case for the event. A",
        "missing case means the event is parsed and dropped, so this column is",
        "asserted, not just reported.",
        "",
        f"| Event | Family | Variant | SPA |",
        "|---|---|---|---|",
    ]
    for name, variant, family in events:
        shown = f"`{family}`" if family else "— (control)"
        lines.append(f"| `{name}` | {shown} | `ChatEvent::{variant}` | ✅ |")
    lines += [
        "",
        "## Handled by the SPA but not defined by `ChatEvent`",
        "",
        "| Event | Why |",
        "|---|---|",
    ]
    for name, why in sorted(NON_CHATEVENT_CONSUMER_CASES.items()):
        lines.append(f"| `{name}` | {why} |")
    lines += [
        "",
        "Anything the SPA does not recognise reaches `onUnknownEvent`, which logs",
        "rather than discarding silently — the runtime counterpart to this file.",
        "",
    ]
    return "\n".join(lines)


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--check", action="store_true", help="fail on drift")
    args = parser.parse_args()

    producer = read(PRODUCER)
    names = parse_arms(fn_body(producer, "pub fn name(&self) -> &'static str"))
    families = parse_arms(fn_body(producer, "pub fn family(&self) -> Option<&'static str>"))

    missing_family = sorted(set(names) - set(families))
    if missing_family:
        sys.exit(
            "these ChatEvent variants have a name() arm but no family() arm, so "
            f"their emission gating is undefined: {missing_family}"
        )

    cases = consumer_cases(read(CONSUMER))
    events = sorted(
        (names[variant], variant, families[variant]) for variant in names
    )

    undropped = [name for name, _, _ in events if name not in cases]
    if undropped:
        sys.exit(
            "these gateway events have NO case in the SPA stream client, so they "
            f"are parsed and silently discarded: {undropped}\n"
            f"Add a `case '<name>':` in {CONSUMER.relative_to(REPO_ROOT)} "
            "(and a handler on the controller), or explain the omission here."
        )

    stale_aliases = [name for name in NON_CHATEVENT_CONSUMER_CASES if name not in cases]
    if stale_aliases:
        sys.exit(
            "this script expects the SPA to still handle these non-ChatEvent "
            f"names, and it does not: {stale_aliases}\n"
            "If they were deliberately removed, drop them from "
            "NON_CHATEVENT_CONSUMER_CASES in this script."
        )

    rendered = render(events)
    if not args.check:
        DOC.parent.mkdir(parents=True, exist_ok=True)
        DOC.write_text(rendered)
        print(f"wrote {DOC.relative_to(REPO_ROOT)} ({len(events)} events)")
        return 0

    current = DOC.read_text() if DOC.exists() else ""
    if current == rendered:
        print(f"sse taxonomy up to date ({len(events)} events)")
        return 0
    diff = "".join(
        difflib.unified_diff(
            current.splitlines(keepends=True),
            rendered.splitlines(keepends=True),
            fromfile=f"committed {DOC.name}",
            tofile="generated from source",
        )
    )
    print(
        f"{DOC.relative_to(REPO_ROOT)} is stale. Regenerate with:\n"
        "  python3 'apps/Model Plane/scripts/gen-sse-event-taxonomy.py'\n\n"
        f"{diff}"
    )
    return 1


if __name__ == "__main__":
    raise SystemExit(main())
