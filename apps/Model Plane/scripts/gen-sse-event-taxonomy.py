#!/usr/bin/env python3
"""Generate (or verify) the SSE chat-event taxonomy from source.

Why this exists
---------------
This repo has a documented, recurring stale-docs problem: prose claiming a
smaller SSE surface than the code emits. The endpoint map is *curated* — its
`v3` relevance ratings and "do not wire yet" flags are human judgment, and
regenerating it would destroy them — so the drift gate belongs on the part that
really is mechanical: the chat event taxonomy.

It checks five things a comment cannot:

1.  **Producer -> consumer coverage.** Every event a chat stream can carry must
    have a matching `case '<name>':` in the SPA's native stream switch
    (`dispatchEvent`). Those events are the `ChatEvent` variants and the literal
    `.event("...")` writes in model-gateway's production code. An event added on
    the backend with no client case is silently dropped: the stream succeeds, the
    feature is invisible, and nothing fails. (The client's `onUnknownEvent` hook
    makes it *loggable* at runtime; this makes it fail at build time, which is
    where it is cheap.)
2.  **Consumer -> producer coverage.** Every native `case` must name something a
    producer writes, so deleting a producer does not leave a dead case behind.
    The AG-UI `CUSTOM` switch is not read: it lives in another function, and its
    lower-case names would make a removed native case look handled.
3.  **Orchestration oneof.** Every `OrchestrationEvent` oneof member has exactly
    one arm in `orchestration_event_to_sse`, and that arm's wire name is the
    proto field name.
4.  **Literal writers.** Every literal `.event("...")` in production code is
    listed in `LITERAL_PRODUCERS` with its reason, and every listed name is still
    written. Test modules are not on the wire and are skipped.
5.  **Doc freshness.** The committed taxonomy doc must equal what source says.

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
from typing import NamedTuple

# apps/Model Plane/scripts/ -> repo root
REPO_ROOT = Path(__file__).resolve().parents[3]
MODEL_GATEWAY_SRC = REPO_ROOT / "apps/Model Plane/rust/services/model-gateway/src"
PRODUCER = MODEL_GATEWAY_SRC / "sse_events.rs"
ORCHESTRATION_MAP = MODEL_GATEWAY_SRC / "sse.rs"
ORCHESTRATION_PROTO = REPO_ROOT / "apps/Model Plane/proto/model_plane/v1/orchestration.proto"
CONSUMER = REPO_ROOT / "apps/Frontend Plane/verevonv3/src/shared/api/chat-client.ts"
DOC = REPO_ROOT / "apps/Model Plane/docs/sse-event-taxonomy.md"

# Frames the SSE layer writes and the SPA reads with no ChatEvent variant.
TRANSPORT = {
    "connected": "transport: stream opened, emitted by the SSE layer",
    "chunk": "transport: assistant text delta",
    "done": "transport: terminal success frame",
}

# Names the SPA handles that the ChatEvent enum does not define, with the reason.
# These are asserted to STILL be handled, so deleting one is also caught.
NON_CHATEVENT_CONSUMER_CASES = {
    **TRANSPORT,
    "citations": "legacy alias retained for older gateway builds",
    "search_results": "legacy alias retained for older gateway builds",
}

# Frames model-gateway writes with a literal `.event("...")` that ChatEvent does not
# name, with the reason. The scan of production source must find exactly these: a
# new literal is an unclassified writer, and an entry nothing writes is stale.
LITERAL_PRODUCERS = {
    **TRANSPORT,
    "error": "shared with `ChatEvent::Error`; the run-events stream writes it directly "
    "when it cannot authorise or open the orchestration stream",
    "awaiting_approval": "agentic run paused for approval at the run boundary, before a "
    "derived `step_update` exists",
    "run_parked": "parked agentic run on the owner's turn stream; the orchestration "
    "`RunParked` event shares the name",
}

VARIANT_ARM = re.compile(
    r"((?:ChatEvent::\w+\s*\{\s*\.\.\s*\}\s*\|?\s*)+)=>\s*(?:Some\(\"(\w+)\"\)|None|\"(\w+)\")"
)
ORCHESTRATION_ARM = re.compile(r"orchestration_event::Event::(\w+)\(_\)\s*=>\s*\{?\s*\"(\w+)\"")
EVENT_LITERAL = re.compile(r"\.event\(\s*\"([A-Za-z0-9_]+)\"\s*\)")
WHOLE_LINE_COMMENT = re.compile(r"^\s*//.*$", re.MULTILINE)
CFG_TEST = re.compile(r"^(\s*)#\[cfg\(test\)\]\s*$")
TEST_ITEM = re.compile(r"^\s*(?:pub(?:\([^)]*\))?\s+)?(?:async\s+)?(?:mod|fn)\s+\w+")
ONEOF_FIELD = re.compile(r"^\s*[\w.]+\s+(\w+)\s*=\s*(\d+)\s*;", re.MULTILINE)
TAG_ASSIGNMENT = re.compile(r"=\s*\d+")
CASE_LABEL = re.compile(r"case '([a-z_]+)':")


class TaxonomyError(Exception):
    """A gate failure, or a parser that matched nothing. Fail loudly, never guess."""


class Taxonomy(NamedTuple):
    events: list[tuple[str, str, str | None]]
    doc: str


def read(path: Path) -> str:
    try:
        return path.read_text(encoding="utf-8")
    except OSError as error:
        raise TaxonomyError(
            f"cannot read {path} ({error}).\n"
            "If the file moved, re-point this script — do not delete the check; "
            "it is the only thing that fails when the backend and the SPA "
            "disagree about which events exist."
        ) from error


def load_rust() -> dict[str, str]:
    """Every model-gateway source file, keyed by its path under `src/`."""
    return {
        path.relative_to(MODEL_GATEWAY_SRC).as_posix(): read(path)
        for path in sorted(MODEL_GATEWAY_SRC.rglob("*.rs"))
    }


def rust_file(rust: dict[str, str], path: Path) -> str:
    key = path.relative_to(MODEL_GATEWAY_SRC).as_posix()
    if key not in rust:
        raise TaxonomyError(f"{key} is missing from {MODEL_GATEWAY_SRC}")
    return rust[key]


def skip_string(source: str, index: int) -> int:
    """Return the index just past the string literal that opens at `index`."""
    quote = source[index]
    index += 1
    while index < len(source) and source[index] != quote:
        index += 2 if source[index] == "\\" else 1
    return index + 1


RUST_QUOTES = '"'
TS_QUOTES = "'\"`"


def block_body(source: str, anchor: str, where: str, quotes: str = RUST_QUOTES) -> str:
    """Return the text inside the first `{...}` after `anchor`.

    Strings and comments are skipped while braces are counted, so a brace inside a
    literal or a comment cannot end the block early. `quotes` lists the characters
    that open a string in this language: Rust lifetimes (`'static`) are not quotes.
    """
    start = source.find(anchor)
    if start < 0:
        raise TaxonomyError(f"`{anchor}` not found in {where} — was it renamed?")
    open_brace = source.index("{", start)
    depth = 0
    index = open_brace
    while index < len(source):
        if source.startswith("//", index):
            newline = source.find("\n", index)
            index = len(source) if newline < 0 else newline
        elif source.startswith("/*", index):
            close = source.find("*/", index + 2)
            index = len(source) if close < 0 else close + 2
        elif source[index] in quotes:
            index = skip_string(source, index)
        else:
            char = source[index]
            if char == "{":
                depth += 1
            elif char == "}":
                depth -= 1
                if depth == 0:
                    return source[open_brace + 1 : index]
            index += 1
    raise TaxonomyError(f"unbalanced braces after `{anchor}` in {where}")


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
        raise TaxonomyError("parsed zero match arms — the parser broke, not the invariant")
    return out


def orchestration_arms(body: str) -> dict[str, str]:
    """Map each OrchestrationEvent oneof variant to the wire name its arm returns."""
    flat = " ".join(strip_line_comments(body).split())
    arms = ORCHESTRATION_ARM.findall(flat)
    written = flat.count("orchestration_event::Event::")
    if not arms or len(arms) != written:
        raise TaxonomyError(
            f"parsed {len(arms)} orchestration arms but the body writes {written}: an "
            "arm has a shape this parser does not read. Extend ORCHESTRATION_ARM; do "
            "not skip the arm."
        )
    variants = [variant for variant, _ in arms]
    duplicates = sorted({variant for variant in variants if variants.count(variant) > 1})
    if duplicates:
        raise TaxonomyError(f"orchestration_event_to_sse has duplicate arms for {duplicates}")
    return dict(arms)


def oneof_fields(proto: str) -> list[tuple[str, int]]:
    """The (field, tag) pairs of `OrchestrationEvent.event`, in source order."""
    message = block_body(proto, "message OrchestrationEvent {", where=ORCHESTRATION_PROTO.name)
    oneof = strip_line_comments(block_body(message, "oneof event {", where="OrchestrationEvent"))
    fields = [(name, int(tag)) for name, tag in ONEOF_FIELD.findall(oneof)]
    assigned = len(TAG_ASSIGNMENT.findall(oneof))
    if not fields or assigned != len(fields):
        raise TaxonomyError(
            f"parsed {len(fields)} oneof fields but the oneof assigns {assigned} tags: a "
            "field has a shape this parser does not read (options, for one). Extend "
            "ONEOF_FIELD; do not drop the field."
        )
    return fields


def camel(field: str) -> str:
    """The Rust variant prost generates for a snake_case proto field."""
    return "".join(part[:1].upper() + part[1:] for part in field.split("_"))


def without_test_items(source: str) -> str:
    """Drop each `#[cfg(test)]` module or function, with its body.

    Test code writes frames no client receives, so its literals are not producers.
    The item must open a block that closes at the attribute's own indentation,
    which rustfmt guarantees. Anything else fails loudly rather than guessing
    where the item ends, because a wrong guess would drop production code.
    """
    lines = source.split("\n")
    kept: list[str] = []
    index = 0
    while index < len(lines):
        attribute = CFG_TEST.match(lines[index])
        if attribute is None:
            kept.append(lines[index])
            index += 1
            continue
        indent = attribute.group(1)
        index += 1
        while index < len(lines) and lines[index].lstrip().startswith(("#[", "//")):
            index += 1
        if index >= len(lines):
            raise TaxonomyError("#[cfg(test)] at the end of a file, with no item after it")
        header = lines[index].rstrip()
        one_line = header.count("{") == header.count("}")
        if one_line and header.endswith((";", "}", "},", "};", "});")):
            # A one-line item such as `mod controls;` or `use x;`: all of it is test-only.
            index += 1
            continue
        if not (TEST_ITEM.match(header) or "{" in header):
            raise TaxonomyError(
                f"unsupported #[cfg(test)] item {header.strip()!r}: expected a mod, fn or "
                "block item. Teach without_test_items this shape; do not guess."
            )
        closing = re.compile(rf"^{re.escape(indent)}\}}[\s;),]*$")
        index += 1
        while index < len(lines) and not closing.match(lines[index]):
            index += 1
        if index >= len(lines):
            raise TaxonomyError(f"#[cfg(test)] item {header.strip()!r} never closes")
        index += 1
    return "\n".join(kept)


def literal_writers(rust: dict[str, str]) -> set[str]:
    """Names written by a literal `.event("...")` outside test modules."""
    found: set[str] = set()
    for text in rust.values():
        production = WHOLE_LINE_COMMENT.sub("", without_test_items(text))
        found.update(EVENT_LITERAL.findall(production))
    return found


def native_cases(consumer: str) -> set[str]:
    """The `case` labels of the native stream switch inside `dispatchEvent`.

    The AG-UI `CUSTOM` switch lives in another function and reuses lower-case
    names, so reading the whole file would count names the native path never
    handles.
    """
    dispatch = block_body(consumer, "function dispatchEvent(", CONSUMER.name, TS_QUOTES)
    native = block_body(dispatch, "switch (event.event) {", "dispatchEvent", TS_QUOTES)
    cases = set(CASE_LABEL.findall(native))
    if not cases:
        raise TaxonomyError(f"parsed zero native `case '...'` labels from {CONSUMER.name}")
    return cases


def render(
    events: list[tuple[str, str, str | None]], orchestration: list[tuple[str, int, bool]]
) -> str:
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
        "## Other producers",
        "",
        "Frames model-gateway writes that `ChatEvent` does not name, read from source: a",
        "literal `.event(\"...\")` in production code (test modules are skipped), and the",
        "`OrchestrationEvent` oneof, whose wire name is its proto field name.",
        "",
        "| Event | Why |",
        "|---|---|",
    ]
    for name, why in sorted(LITERAL_PRODUCERS.items()):
        lines.append(f"| `{name}` | {why} |")
    lines += [
        "",
        "The run-events stream (`/v1/runs/:run_id/events`) carries the orchestration",
        "events. Tags are the proto field numbers. `run-console-client.ts` reads them;",
        "this gate checks only the producer side. A member with no arm is declared on",
        "the wire but not written yet.",
        "",
        "| Event | Tag | Written |",
        "|---|---|---|",
    ]
    for field, tag, written in sorted(orchestration, key=lambda item: item[1]):
        lines.append(f"| `{field}` | {tag} | {'✅' if written else 'not yet'} |")
    lines.append("")
    return "\n".join(lines)


def build(rust: dict[str, str], proto: str, consumer: str) -> Taxonomy:
    producer = rust_file(rust, PRODUCER)
    names = parse_arms(
        block_body(producer, "pub fn name(&self) -> &'static str", PRODUCER.name)
    )
    families = parse_arms(
        block_body(producer, "pub fn family(&self) -> Option<&'static str>", PRODUCER.name)
    )
    missing_family = sorted(set(names) - set(families))
    if missing_family:
        raise TaxonomyError(
            "these ChatEvent variants have a name() arm but no family() arm, so "
            f"their emission gating is undefined: {missing_family}"
        )
    arms = orchestration_arms(
        block_body(
            rust_file(rust, ORCHESTRATION_MAP),
            "fn orchestration_event_to_sse(",
            where=ORCHESTRATION_MAP.name,
        )
    )
    members = oneof_fields(proto)
    cases = native_cases(consumer)
    literals = literal_writers(rust)

    events = sorted((names[variant], variant, families[variant]) for variant in names)
    chat_names = {name for name, _, _ in events}
    problems: list[str] = []

    undeclared = sorted(literals - set(LITERAL_PRODUCERS))
    if undeclared:
        problems.append(
            "these literal `.event(\"...\")` writes in model-gateway have no entry in "
            f"LITERAL_PRODUCERS, so nothing records why they exist: {undeclared}"
        )
    stale_literals = sorted(set(LITERAL_PRODUCERS) - literals)
    if stale_literals:
        problems.append(
            "LITERAL_PRODUCERS lists these names, but no production source writes them "
            f"any more: {stale_literals}"
        )

    undropped = sorted((chat_names | set(LITERAL_PRODUCERS)) - cases)
    if undropped:
        problems.append(
            "these gateway events have NO case in the SPA stream client, so they "
            f"are parsed and silently discarded: {undropped}\n"
            f"Add a `case '<name>':` to dispatchEvent in {CONSUMER.relative_to(REPO_ROOT)} "
            "(and a handler on the controller), or explain the omission here."
        )

    stale_aliases = sorted(name for name in NON_CHATEVENT_CONSUMER_CASES if name not in cases)
    if stale_aliases:
        problems.append(
            "this script expects the SPA to still handle these non-ChatEvent "
            f"names, and it does not: {stale_aliases}\n"
            "If they were deliberately removed, drop them from "
            "NON_CHATEVENT_CONSUMER_CASES in this script."
        )

    known = (
        chat_names
        | set(NON_CHATEVENT_CONSUMER_CASES)
        | set(LITERAL_PRODUCERS)
        | set(arms.values())
    )
    unknown = sorted(cases - known)
    if unknown:
        problems.append(
            "the SPA stream client handles these names, but no producer writes them: "
            f"{unknown}\nRemove the case, or record the producer it serves in this script."
        )

    expected = {camel(field): field for field, _ in members}
    orchestration_problems = [
        f"arm '{variant}' names no OrchestrationEvent member"
        for variant in sorted(arms)
        if variant not in expected
    ] + [
        f"arm '{variant}' returns '{arms[variant]}' but its proto field is '{field}'"
        for variant, field in sorted(expected.items())
        if variant in arms and arms[variant] != field
    ]
    if orchestration_problems:
        problems.append(
            "orchestration_event_to_sse does not match the OrchestrationEvent oneof:\n"
            + "\n".join(f"  {item}" for item in orchestration_problems)
        )

    if problems:
        raise TaxonomyError("\n\n".join(problems))
    written = {field for variant, field in expected.items() if variant in arms}
    rows = [(field, tag, field in written) for field, tag in members]
    return Taxonomy(events, render(events, rows))


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--check", action="store_true", help="fail on drift")
    args = parser.parse_args()
    if hasattr(sys.stdout, "reconfigure"):
        # The doc carries a check mark. A legacy console code page must not turn a
        # legible drift diff into a UnicodeEncodeError traceback.
        sys.stdout.reconfigure(encoding="utf-8", errors="replace")

    try:
        taxonomy = build(load_rust(), read(ORCHESTRATION_PROTO), read(CONSUMER))
    except TaxonomyError as error:
        sys.exit(str(error))
    rendered = taxonomy.doc
    count = len(taxonomy.events)

    if not args.check:
        DOC.parent.mkdir(parents=True, exist_ok=True)
        DOC.write_text(rendered, encoding="utf-8")
        print(f"wrote {DOC.relative_to(REPO_ROOT)} ({count} events)")
        return 0

    current = DOC.read_text(encoding="utf-8") if DOC.exists() else ""
    if current == rendered:
        print(f"sse taxonomy up to date ({count} events)")
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
