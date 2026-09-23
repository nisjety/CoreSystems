# Recording pass 4: project acceptance and deadline audit

**Release remains blocked.** The customer, sales and campaign films from [pass 3](PRODUCT_RECORDING_CAPTURE_PASS3_2026-09-22.md) are private editorial rehearsals. The project scenario still has no complete passing initial request, revision, reload or film. Every live inference in this pass used `gpt-5.6-terra` through `openai-codex-subscription`; no Claude, Verevon Balanced or API fallback was used.

## Verified corrections

- The local approved-date checker no longer treats the substring `er godkjent` inside `krever godkjent` as an affirmative approval. It also recognizes `forslag` as a proposal qualifier. Regression controls cover both that false positive and a genuine unsupported approval.
- Project author guidance now distinguishes a pilot target from a decision date, work ownership from approval authority, completion criteria from release gates, and source-stated prerequisites from invented ones.
- The Playwright project oracle no longer rejects the truthful status cell `Mulig dato, ikke vedtatt` merely because the row contains a date and the word `vedtatt`. It checks the status cell and still rejects an affirmative `Vedtatt` status. The two direct regression cases, 34 existing recording-policy tests and scoped strict TypeScript all pass (36 focused frontend tests total).

One separately labelled v17 diagnostic produced a source-accepted initial project document after one reviewer-suggested edit and a complete independent recheck. The old browser regex then falsely failed on `ikke vedtatt` before the revision began. That diagnostic is not a qualified run. After the browser oracle correction, a fresh normal v17 take reached source review but timed out in its recheck.

## Candidate and live evidence

| Candidate | Complete project result | Observed source-review path |
| --- | --- | --- |
| v17, normal after oracle fix | Failed | First review 50.024 s, two unsupported segments patched inline, then the full recheck exceeded the 90 s artifact deadline. |
| v18, smaller groups only for longer plans | Failed | The authored document had 21 segments and stayed on four groups. First review 60.286 s, four edits, then recheck timed out. |
| v19, groups selected by review workload | Failed | First review 52.151 s found four unsupported segments; author repair took 20.200 s; full recheck timed out. |
| v20, six-way grouping and priority recheck | Failed | First review 65.062 s found four unsupported segments; inline patches were applied; only three of six recheck groups finished before the artifact deadline. |

All four normal attempts are retained with browser reports and raw failed recordings under the private `recordings-pass6` evidence directory. A failed or timed-out source review never published an artifact or became a passing take. The v17/v18/v19 stage timings above were observed in gateway logs during each run; the v20 filtered stage events are saved alongside its browser report. Browser errors were `response_validation_timeout`, not evidence that a finished plan was flawless.

The gateway unit suite passed on the experimental candidates (v20: 1,280 passed, one ignored). The v20 live context/repair controls were 10/10 correct, and the seven difficult high-effort semantic controls were 7/7 correct. A separate low-effort **experiment** completed the full 104-case semantic matrix at **103/104**: it falsely rejected a valid conditional project date that had passed in the focused run. That setting is disqualified. Neither the six-way grouping nor the priority recheck yielded a complete project take, so both were removed. `source_validation.rs` now matches the validated v17 source bytes; first and subsequent semantic reviews again use high effort and at most four concurrent groups.

The previously completed v16 full matrix remains the most recent **passing complete** semantic matrix: 104/104, with 10/10 context/repair controls. The v20 component results are retained as experimental evidence, not promoted to a release claim. All 13 business fixture hashes still match the manifest; their prompts and source packs were not changed to make the tests pass.

## Remaining gates

The project artifact validator needs a way to finish source-bounded repair and independent recheck inside its existing deadline without weakening factual judgment. The current first-review critical path varies from roughly 50 to 65 seconds on ordinary project drafts, and the draft often has two to four unsupported claims. A review protocol that rechecks the corrected document efficiently, while detecting changed antecedents and cross-section conflicts, needs separate accuracy and latency evidence before another release candidate is selected.

Then capture a complete project initial request, exact REGI follow-up, checked artifacts and server reload; inspect the raw take and prepare a private film only from that passing footage. After a single build is selected, establish **five consecutive qualified complete runs per scenario** and finish performance, recovery, durability, editorial and device checks. All public media fields remain null, all four qualification counts remain zero, and publication approval remains false. Three existing films run on different earlier builds and cannot substitute for that release gate.
