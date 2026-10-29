# Adversarial review: patch-style revision design

Read-only. I wrote no files. Repo paths are relative to `C:/dev/CoresSystem/apps/Model Plane/rust/services/model-gateway/src` (`tool_loop.rs`, `sse.rs`, `source_validation.rs` and so on) unless a full path is given. I re-derived the 03 numbers from `q16e-turn-timing.jsonl` and `q16f-turn-timing.jsonl` (n=30 with draft over 25 s; p50 43.9 s, p95 nearest-rank 76.3 s, max 90.9 s, matching the map). I also read the 30 recorded 03 initial and revised documents under `C:/private/verevon-recordings/q16e-20260928T0202Z` and `q16f-20260928T1814Z`.

## Verdict

- **No-go on `patch_artifact` as v1.** It costs 8-10 engineer-days and adds an exact-quoting failure class. It needs a new review-reuse policy, and its live oracles cannot detect the regression that matters.
- **Go on a cheaper first stage:** for the already-recognised dated-post revision, have the model emit only its section, and let the existing `Preservation::apply` splice it. It reuses the existing scope rulings and needs no owner ruling on scope. Details are in sections 5 and 6.

## 1. Anchor verification

I checked 20 or more claims against the code. Most are exact.

| Claim | Result |
|---|---|
| `tool_loop.rs` 4132-4142 (discipline notice), 4151, 4192, 4210, 3846, 3853, 313, 345, 4572 | Correct |
| 186 (`MAX_TOOL_OUTPUT_CHARS`=8000) with 4465 and 4403-4419 (`bounded_tool_output` applied to every result, including the prefetch) | Correct. The model sees at most 8000 characters of a document the store reads up to 24000 (4147, 2782). |
| 6559 `round_phase` fed from offered tool names; `turn_timing.rs` 366 | Correct. `patch_artifact` must be added or `FirstDraft` is lost. |
| 6804 `remember_review`, 6839 content strip, 6868 dispatch guard, 6961 scoped completion, 7091 `tool_failures`, 7101 and 7116 source-read flags | Correct (off by 0-1 line) |
| `sse.rs` 1783-1794 fork, 3374-3392 lock and `unchanged_from`, 3665-3679 `adopt_review`, 4910 rehydrate (no reviews restored) | Correct |
| `source_validation.rs` 897, 474, 745, 810, 1390, 1462, 2195, 2337, 2448, 2508, 2606, 2764; CHECKER v16 at line 24 | Correct |
| `artifacts.rs` 92, 219, 387, 398, 455 | Correct |
| `author_guidance.rs` mentions no artifact tools (0 hits) | Correct |
| Frontend never reads `reusedSegments`, has no `update_artifact` reference, and `ChatArtifactPanel.tsx` line 205 calls `diffText` | Correct |
| `apps/Model Plane/deploy/docker-compose.yml` line 292 | Correct, but the repo pattern is `${VAR:-}`, not `:-off` |

**Wrong or inexact claims:**

- **W1, injection order impossible.** The design says to compile "after the truncation check, before `rewrites_validated_document`". In the code the rewrite drop is at 6582-6600 and `truncated_index` is computed later at 6636-6650. Only the boolean `last_call_truncated` (6574) exists before the drop. The compile step must go before 6582 and use the last-call rule, or after 6650 and skip that guard.
- **W2, "complete read" flag and `seen_reads`.**
  - `sole_revision_source_read` (7116) keys on the read's own 24k marker, not on the 8000-character view. `seen_reads.complete` must be recomputed from the character count.
  - After the first whole read, `read_artifact` is removed from `round_tools` for the rest of the turn (6367-6373). The design says a second patch "requires a fresh read", which is then impossible. The flag must reset when the version changes.
- **W3, "P1 reproduces today's set exactly" is true only for `##` posts.**
  - `revision_preservation::sections()` treats a standalone `**Label**` line as a level-6 heading. 22 of 30 recorded initial 03 documents have `**Brødtekst:**` in the E-post section. An email-body edit would then get a piece that excludes the subject and preview lines.
  - An edit inside a nested `### Kilder` gets a narrower piece than the dated-post piece.
  - `sections()` and `Section` are private.
  - Fix: use ATX headings only, and take the outermost non-H1 ancestor.
- **W4, cost of a failed patch is understated (10-12 s).** The gateway awaits the whole unary `infer` (about 6485-6520), so a refusal is known only after the full patch is generated. That is about 15 s per failure, not 10-12 s.
- **W5, the kill threshold is too lax.** See section 5.
- **W6, the lenient normaliser omits Markdown hard breaks** (two trailing spaces before a newline). 6 of 30 recorded documents contain them, and "newlines kept" does not say how they are handled.
- **Minor:** the campaign contract guidance is pushed once before the loop (6243-6245), not every round.

## 2. Meaning changes the changed-segment review would miss

Baseline mechanics that matter:

- Cross-turn `revision_carryover` marks a segment outside the piece as dependent only if it shares a changed entity. Entities are dates, grouped numbers, three-digit numbers and proper names.
- It lacks the `heading_context` inequality, the aggregate-row rule and the conclusion rule that `snapshot_carryover` has.
- The reviewer sees the whole document, but only for assigned segments.

| Case | Why it escapes | What the design must add |
|---|---|---|
| Stale summary. Measured in 03 q16f take 02: the post now says "Vinkel: Delt kontor og konkrete produktvalg" while the unchanged overview row says "Justering av lampens arm og valg mellom lysnivåer". Take 03 shows the same divergence. | The overview row shares only the unchanged date. It is carried unreviewed. Today's dated-post ruling already accepts this, but a patch extends it to arbitrary edits. | Owner decision D3. Either accept the status quo, or add the edited section's heading and key entities as dependents. That costs about one extra segment, roughly 9 s. |
| Table or list total in another section | The total repeats none of the edited numbers. Section-level freshness covers it only when it sits in the same section. | Port the `edited_groups` and `is_aggregate_row` rule, grouped by table header rather than heading. Ideally add a local arithmetic check. |
| Renamed or replaced term that is not an "entity" | Lowercase nouns and sentence-initial names are not entities. "1 000 kroner" becoming "1 000 euro", or "2 år" becoming "2 måneder", leaves the numeric key unchanged. | Term-level dependents. From each edit's old and new spans, take the removed content tokens and mark carried segments containing them as fresh. If more than 8 segments are marked, drop reuse and review everything. |
| Heading scope | A patch can insert a heading through `newText`, or change a table header, in an edit anchored on ordinary text. The design's ineligibility rule looks only at old spans. | Compare the heading sequence (level and label) and table headers between before and candidate. Any difference means no reuse. Also add `heading_context` inequality as a dependency. |
| Caveat or status deleted | Review checks the assertions that remain, not the absence of a caveat. Carried segments stay approved under the old framing. The H1 preamble is covered by the 50% rule; a mid-document caveat is not. | An edit whose old or new text contains qualifier or status tokens (utkast, forslag, ikke bekreftet, planlagt, publisert, sendt, godkjent, draft, proposal and similar) forces a full review. |
| Back-reference or conclusion at the top of the next section | `needs_fresh_reference_check` and `opens_with_conclusion` exist only on the in-turn path. | Apply them to the first segment after each edited piece. |
| Cumulative drift | `receipt.evidence` includes carried evidence, so an approval can persist through many versions with no age limit. | Owner decision D4. Cap the chain depth (for example 2) or force a full fresh review every N revisions. |
| Deleting a required section (Kilder, Status, notes) | Deletion is allowed. Review checks claims, not presence. | A structure invariant. The heading sequence and the Status and Kilder blocks must survive unless the instruction has a removal verb. |
| Duplicate identical segments (4 identical Status lines) | Alignment is by text, so a duplicate can be aligned to the wrong slot. | Keep the design's span-aware alignment, and never carry a segment that overlaps an edit. |

## 3. Abuse and failure paths

- **Quoted source lines, blockquotes, fences.** An edit whose span touches one must make reuse ineligible. A unique match inside a quote is still a quote rewrite, and the fresh review must check fidelity.
- **Receipt metadata.** The receipt lives outside the document body, in the SSE `result_receipt` event and the message's `artifactChecks`. The `patch` object must be built only from hashes, ranges and counts, never model text.
- **Audit gap.** The audit trail would record only the compiled `update_artifact` with the full spliced content. Record the patch hash and the model-proposed edit count too.
- **Internal notes.** `InternalNotes` preservation discards edits in the notes. An unrecognised instruction leaves the notes editable, with fresh review only.
- **Title fallback.** Compilation must resolve by id only. `id_for_title` and `resolve_similar_id` could aim a patch at a different artifact than the one the model read.
- **Prompt injection through document text.**
  - The document is already data (`format_gateway_read_context`).
  - Patch adds a targeting channel, but it is bounded by 12 edits and fresh review of the touched pieces.
  - Refusal messages that echo "nearest passage" must be bounded and framed as data.
- **Repeated identical failing patches.** `duplicate_call_signature` (2287) covers only fetch and search tools. Add a signature for patches.
- **Restart or second replica.** The first follow-up reviews everything. Rehydrating persisted receipts needs an integrity tag (for example an HMAC by the gateway). Otherwise forged metadata could unlock reuse.

## 4. Does the A/B plan detect a correctness regression?

No.

- **The e2e oracles are vacuous for this change.** `tests/e2e/product-scenarios.spec.ts` 253-257 assert that the 21 and 25 September posts and the email are unchanged. `Preservation::apply` already guarantees that today (`revision_preservation.rs` 205-232), and a splice would too. The 4 document checks are local. The `checker` string is a constant. Nothing asserts which segments were reviewed.
- **The rejection-rate criterion points the wrong way.** Under-coverage lowers rejections.
- **p95 at n=20 is the second-worst sample.** The harness reports nearest-rank: q16e 04 with n=18 shows p95 equal to max at 74.6 s. Two slow takes fail the target, so one 20-take arm comparison is mostly noise.

**Minimal live tests:**

- **T1, offline replay, free.** Run scope derivation on the 30 recorded (initial, revised) pairs and require the reviewed set to equal the receipt's set. This proves or refutes "reproduces exactly" before any live run.
- **T2, adoption smoke, n=10.** For the chosen output mode, measure the fraction of turns where the model emits the intended shape and the draft size in characters.
- **T3, seeded dependency escapes.** Cases: stale overview, caveat flip, heading insertion, total in another section, currency change. Each one is followed by a no-reuse shadow review of the published version. Require zero carried-but-rejected segments.
- **T4, quoting corpus, only if patch is pursued.** 30 single-turn calls on fixtures with hard breaks, en dashes, «», tables and NBSP.
- **T5, interleaved 03 A/B.** Use paired `draft_ms` as the primary metric. Confirm p95 with n≥30 and report the bootstrap pass probability.

## 5. Are the savings realistic, and is there a cheaper alternative?

**Draft model.** Draft time is about 9 s plus 6 ms per output character. This is corroborated by three other journeys: 02 (0.6 KB, about 11 s), 04 (0.3-0.6 KB, 8-16 s) and 01 (1.2 KB, about 15 s).

**Patch size.** In all 30 recorded 03 pairs exactly one paragraph changed. Old plus new averages 974 characters, so about 1.0-1.2 KB with JSON escaping, or roughly 15 s. That matches the design.

**Projection.** I resampled the 30 measured takes, replacing the draft. A failed patch costs the full patch (15 s) before it is detected, and a second failure falls back to a 32.5 s full update. P(pass) below is the chance that a 20-take arm has p95 (nearest-rank) at or below 70 s.

| Mode | p50 | p95 (n=30) | P(pass) at n=20 |
|---|---|---|---|
| Baseline | 43.9 s | 76.3 s | 23% |
| Patch, 100% first-try success | 25.7 s | 62.7 s | 86% |
| Patch, 90% first-try success | 25.9 s | 62.7 s | 78% |
| Patch, 80% first-try success | 26.9 s | 74.0 s | 55% |
| Patch, 70% first-try success | 30.1 s | 74.4 s | 26% |
| Patch, 50% first-try success | 39.2 s | 83.8 s | 1% |
| Section-only output (12.5 s draft) | 23.3 s | 60.2 s | 86% |

So the design's 80% first-try kill threshold means the target is roughly a coin flip. It should be at least 95%.

**Cheaper alternative, section-only output.** For a recognised dated post, the model sends `update_artifact` with only the revised `## ...` section.

- `Preservation::apply` already takes the candidate's section and splices it into `before`. A candidate that is only the section works as is.
- The full document then flows through the local checks and `check_artifact` unchanged.
- Add a per-turn system message and a small tolerance: if the fragment has no heading, prepend the original heading. This is Stage 1 in section 6.
- Output is about 0.55 KB. Draft is about 12.5 s, saving about 20 s, against about 17.5 s for a patch.
- There is no oldText, uniqueness or whitespace failure class. Scope stays the instruction piece, so no new ruling. No new tool name means none of the name-list sites, `round_phase`, `writes_document` or dispatch guards are touched.
- Effort is S, against L for the patch.
- If the model sends a full document anyway, the result equals today's behaviour.

**Generalising later.** For unrecognised instructions, "changed sections plus explicit unchanged-section placeholders" beats oldText patches on reliability:

- The gateway validates each placeholder against an outline of existing labels and expands it from `before` byte for byte.
- There is no verbatim old-text reproduction.
- Sub-section granularity is lost, which is irrelevant here (sections are about 500 characters).
- It needs the same scope decision D2 and the same closure items as the patch.

**Not covered by any draft-size change:**

- Heavy review batches and rejection loops. Four of 30 03 follow-ups exceed 70 s. The two review-loop tails in q16e (76 s and 91 s) remain near or above the target.
- Journey 04. Its q16e follow-up p95 was 74.6 s (n=18). It is a review tail, untouched by the draft.
- The per-batch review floor of about 9 s.

## 6. Recommendation, order and owner decisions

**No-go on `patch_artifact` v1. Go on the staged plan:**

1. **Stage 0 (S).**
   - Log draft input and output tokens at 6557-6566, content-free.
   - Fix the 8000 versus 24000 mismatch. Gate the prefetch and any whole-document rewrite on what the model can actually see. This is a latent correctness issue that exists today for documents over 8000 characters.
2. **Stage 1 (S, 1-2 days).**
   - Section-only output for the recognised dated post, behind a default-off lever.
   - Tests: fragment splice byte identity, missing-heading tolerance, full-document fallback unchanged.
   - Run T1, T2 and T5. Kill if fragment adoption is under 90%.
3. **Stage 2 (decide on data).**
   - Address the review tails: 03 heavy batches and rejection loops, and 04's tail.
   - Widen `Preservation::from_prompt` recognition only if other journeys need it.
   - Build placeholders or patch only if p95 still misses after Stage 1. Add the section 2 closure items first.

**Owner decisions needed:**

- **D1.** Confirm Stage 1 needs no new scope ruling, because the model emitting only the piece equals today's "output outside the piece is discarded".
- **D2.** Only for generalisation. May an edit-derived piece serve as reuse scope for unrecognised instructions? Recommend no until Stage 1 is measured. If yes, require the section 2 closure items.
- **D3.** Stale-summary tolerance: keep the status quo, or add heading and key-entity dependents at about +9 s per revision.
- **D4.** An age cap on carried approvals.
- **D5.** Raise the first-try success bar to at least 95%. Adopt paired `draft_ms` plus bootstrap pass probability with n≥30 as the acceptance statistic.
- **D6.** Confirm the p95 definition: nearest-rank, with two outliers failing at n=20.

## 7. Outcome of Stage 1, 2026-09-29

Stage 1 was built as staged (draft-size logging, the visibility fix, and the section-only switch) and measured live in Q16h: 20 takes of journey 03 on the Terra subscription route. Numbers are in the [readiness note](../../product-recordings/evidence/PRODUCT_RECORDING_READINESS_2026-09-24.md).

- **T2, adoption:** 20 of 20 follow-ups came back as a section, against the 90% kill criterion. The model wrote 474 to 623 characters for a document of about 3.6 KB, and the draft was ready at a median of 14.3 s instead of about 32 s.
- **T5, timing:** follow-up p50 26.7 s, p95 41.6 s, none over 70 s, against a p95 of 76 s before. **What this is not:** a single arm of 20, compared with the earlier series, not interleaved, with no paired `draft_ms` and no bootstrap pass probability. The D5 statistic is not met; the margin to the 70 s target is large enough that it does not change the conclusion.
- **Section 4 was right.** A correctness regression appeared that no oracle could see: the splice replaces the target post's whole section, so a section sent without its `### Kilder` sub-section deleted the note. It happened in 3 of 3 takes that had one. No copy check failed, because the checks measure the customer copy. It was found by comparing the initial and revised documents. The fix (told, refused and repaired, judged by a new oracle check) held live in Q16j on 2026-09-30: 4 of 4 takes with a note kept it, and the telling alone was enough, so the refusal has not fired live yet. The first attempt at that series stopped on the subscription's usage limit until the owner reset it.
- **Not done:** T3 (seeded dependency escapes), which concerns the reuse scope and not the sub-section loss; T4, because no `patch_artifact` was built; D2, the generalisation, which stays "no".
- **Decision taken:** the switch is on by default in compose. `MODEL_GATEWAY_DATED_POST_SECTION_ONLY=0` in the environment returns to whole-document output without a rebuild.