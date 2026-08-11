# Quarry Scoreboard

Cycle 28 / cluster #12.

> **Auto-generated.** Edit the suites in `crates/quarry-core/src/benchmark.rs::builtin_suites()`.
> The local dry-run layout is emitted with `cargo run -p quarry-evals --bin
> quarry-eval -- scoreboard`.

## Latest run

| Suite                         | Bucket           | Quarry v2 | Trafilatura | Mozilla R. | Quarry v1 |
| ----------------------------- | ---------------- | --------- | ----------- | ---------- | --------- |
| `bench-static-html`           | static-html      | _pending_ | _pending_   | _pending_  | _pending_ |
| `bench-js-heavy`              | js-heavy         | _pending_ | _–_         | _–_        | _–_       |
| `bench-bot-sensitive`         | bot-sensitive    | _pending_ | _–_         | _–_        | _pending_ |
| `bench-change-tracking-gold`  | change-tracking  | _pending_ | _–_         | _–_        | _–_       |

`_pending_` = corpus + harness defined but a live run hasn't been
checked in. `_–_` = baseline doesn't participate in this suite.

## How scores are computed

| Metric                  | Type            | Suites where it's the primary       |
| ----------------------- | --------------- | ----------------------------------- |
| `markdown_quality`      | higher-better   | bench-static-html                   |
| `extraction_completeness` | higher-better | bench-js-heavy                      |
| `block_rate`            | lower-better    | bench-bot-sensitive                 |
| `change_detection_f1`   | higher-better   | bench-change-tracking-gold          |

`markdown_quality` is a F1 score against a hand-curated gold
extraction for every corpus URL. `extraction_completeness` is
percentage of expected DOM nodes captured. `block_rate` is the
percentage of corpus URLs that returned 4xx/5xx/CDN-challenge.
`change_detection_f1` measures true-positives / false-positives
against a gold-set of known content changes.

## Release gating

Cluster #12's acceptance criterion:

> Every release candidate runs benchmark comparisons; scoreboard
> validates performance claims.

The CI gate compares the candidate's scorecard to the last released
version's scorecard:

| Delta on a higher-better metric | Verdict |
| ------------------------------- | ------- |
| ≥ +5%                            | `Pass`  |
| −5% to +5%                       | `Pass`  |
| −5% to −15%                      | `Warn`  |
| ≤ −15%                           | `Fail`  |

Delta on a lower-better metric uses the same bands inverted.

## How to add a suite

1. Add a `BenchmarkSuite { … }` to `builtin_suites()` in
   `crates/quarry-core/src/benchmark.rs`.
2. Implement the score computation in `lab/evals` (cycle 29 follow-up).
3. Run the suite locally: `cargo run -p quarry-evals --bin quarry-eval -- scoreboard`.
4. Submit a scoreboard PR with the recorded `ScorecardEntry`.

## Remaining benchmark work

- Add the Spider-rs adapter after its licence, isolation, and security review.
- Implement the remaining local baselines and fixture/gold scoring, then run
  the credential-gated external jobs from a protected release workflow.
- This file becomes auto-generated from the latest scorecard rows.
