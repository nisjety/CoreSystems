#!/usr/bin/env python3
"""Fit the confidence bands from rated turns.

Every threshold in model-gateway's `confidence.rs` was placed by hand from a
probe of eight answers whose truth the author happened to know. That was enough
to find the SHAPE of the signal (fabrications collapse on the claim statistic,
correct content does not) and nowhere near enough to pin the edges. This reads
the labelled rows the gateway emits when a user rates a turn and reports where
the edges actually belong.

Usage:
    docker logs model-plane-model-gateway-1 2>&1 | python calibration_report.py
    python calibration_report.py path/to/log [more logs...]

A row is emitted once per rated chat turn (`calibration.rs::emit_sample`) and
carries only numbers plus the rating — no question or answer text — so these
logs can be collected and kept without carrying tenant content.

Reads nothing but stdin/files and writes nothing: the fit is a recommendation
for a human to apply, not a knob this turns.
"""
from __future__ import annotations

import fileinput
import json
import sys
from collections import Counter, defaultdict

MARKER = "calibration_sample"
# The bands as they stand, so the report shows current-vs-fitted rather than a
# number with no reference. Keep in step with `certainty_adjustment`.
CURRENT_BANDS = [
    (0.95, "+0.16 (and the 0.90 floor)"),
    (0.80, "+0.08"),
    (0.45, "neutral"),
    (0.25, "-0.06"),
    (0.00, "-0.12"),
]
# A rating of `poor` is the negative label; `good` and `acceptable` are not.
NEGATIVE = {"poor"}


def rows(streams):
    """Yield the sample dicts from log lines, whatever wrapper they arrive in."""
    for line in streams:
        if MARKER not in line:
            continue
        # tracing's JSON layer nests the values under `fields`; the plain text
        # layer prints them as key=value. Try JSON, then fall back.
        record = None
        start = line.find("{")
        if start != -1:
            try:
                parsed = json.loads(line[start:])
                record = parsed.get("fields", parsed)
            except json.JSONDecodeError:
                record = None
        if record is None:
            record = {}
            for token in line.split():
                key, sep, value = token.partition("=")
                if sep:
                    record[key] = value.strip('"')
        if record.get("sample") != MARKER and MARKER not in line:
            continue
        yield record


def as_float(value):
    try:
        return float(value)
    except (TypeError, ValueError):
        return None


def summarize(samples):
    if not samples:
        print(
            "No calibration rows found.\n"
            "Rows appear only once users rate chat turns; if the plane has been\n"
            "restarted since the last rating, its in-memory rows are gone."
        )
        return

    print(f"{len(samples)} rated turns\n")

    by_rating = Counter(s.get("rating") for s in samples)
    print("ratings: " + ", ".join(f"{k}={v}" for k, v in sorted(by_rating.items())))

    # The question the bands answer: given this claim probability, how often was
    # the answer actually bad? An edge belongs where that rate changes.
    scored = [
        (as_float(s.get("claim_probability")), s.get("rating"))
        for s in samples
        if as_float(s.get("claim_probability")) is not None
    ]
    if not scored:
        print(
            "\nNo rows carry a claim probability — the serving model reports no\n"
            "logprobs (Anthropic), so there is nothing to fit here yet."
        )
        return

    print(f"\n{len(scored)} of them carry a claim probability.\n")
    print("claim probability  n     % rated poor   current band")
    print("-----------------  ----  -------------  ------------")
    buckets = defaultdict(list)
    for probability, rating in scored:
        # Deciles: fine enough to see an edge move, coarse enough to have
        # counts in each row.
        buckets[min(int(probability * 10) / 10, 0.9)].append(rating)
    for low in sorted(buckets, reverse=True):
        ratings = buckets[low]
        poor = sum(1 for r in ratings if r in NEGATIVE)
        band = next(effect for edge, effect in CURRENT_BANDS if low >= edge)
        share = f"{100 * poor / len(ratings):5.1f}%" if ratings else "    —"
        print(f"  {low:.1f} - {low + 0.1:.1f}      {len(ratings):<4}  {share}        {band}")

    thin = [low for low, ratings in buckets.items() if len(ratings) < 20]
    if thin:
        print(
            f"\n{len(thin)} of {len(buckets)} deciles have fewer than 20 samples. Treat"
            "\ntheir rates as noise, not as an edge — collect more before moving a band."
        )

    # The one comparison that justifies grading on claim tokens rather than the
    # whole answer. If the whole-answer statistic separates ratings just as
    # well, the extra field is not earning its place.
    both = [
        (as_float(s.get("claim_probability")), as_float(s.get("whole_probability")), s.get("rating"))
        for s in samples
        if as_float(s.get("claim_probability")) is not None
        and as_float(s.get("whole_probability")) is not None
    ]
    if both:
        poor = [(c, w) for c, w, r in both if r in NEGATIVE]
        fine = [(c, w) for c, w, r in both if r not in NEGATIVE]
        if poor and fine:
            claim_gap = sum(c for c, _ in fine) / len(fine) - sum(c for c, _ in poor) / len(poor)
            whole_gap = sum(w for _, w in fine) / len(fine) - sum(w for _, w in poor) / len(poor)
            print(
                f"\nSeparation between well-rated and poorly-rated answers:\n"
                f"  claim statistic  {claim_gap:+.3f}\n"
                f"  whole answer     {whole_gap:+.3f}\n"
                "A larger gap is a better discriminator. If the whole-answer number\n"
                "matches or beats the claim number here, revisit grading on claims."
            )

    # Did verification earn its cost? Only rated turns that actually ran it.
    verified = [s for s in samples if s.get("verdict")]
    if verified:
        print("\nverification outcomes on rated turns:")
        for verdict, count in Counter(s["verdict"] for s in verified).most_common():
            poor = sum(1 for s in verified if s["verdict"] == verdict and s.get("rating") in NEGATIVE)
            print(f"  {verdict:<12} n={count:<5} {100 * poor / count:5.1f}% rated poor")
        print(
            "  'supports' should be the least often rated poor. If it is not, the\n"
            "  judge is confirming answers the sources do not actually back."
        )


def main():
    streams = fileinput.input(files=sys.argv[1:] or ("-",))
    summarize(list(rows(streams)))


if __name__ == "__main__":
    main()
