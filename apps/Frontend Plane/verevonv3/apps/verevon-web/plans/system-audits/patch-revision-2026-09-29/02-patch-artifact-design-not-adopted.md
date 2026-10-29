# Patch-style revision path for follow-up latency

Read-only design. I re-verified the revision map against the code in `C:/dev/CoresSystem/apps/Model Plane/rust/services/model-gateway/src`. Six anchors the design depends on, some new, are in the last section.

## 0. Decisions

1. **Sibling tool `patch_artifact`, not an extended `update_artifact`.**
   - With the lever off, the advertised tools and prompts stay byte-identical to today.
   - `update_artifact` stays the explicit full-content fallback.
   - The tool schema avoids top-level `anyOf`, which some providers reject.
2. **Compile, don't dispatch.**
   - Right after the model's round arrives, the gateway resolves each valid `patch_artifact` call against the exact current bytes and rewrites it to `update_artifact {id, content: spliced, title?}`.
   - Everything downstream stays as it is: `check_artifact`, `remember_review`, dispatch, events, `lock_commit`, `adopt_review` and the frontend.
   - The audit action still says `update_artifact`, so no allow-list changes are needed.
3. **Review scope derived from the edit.**
   - Policy P1 (v1): the fresh-review set is every segment in the heading section(s) enclosing an edit, plus entity dependents outside them.
   - This is the existing 2026-09-27 dated-post ruling, with the "piece" taken from the edit instead of from the instruction. For journey 03 it reproduces today's review set exactly. I recommend it as needing no new ruling; the owner should confirm that reading.
   - Policy P2 (changed segments plus `snapshot_carryover` dependents only) needs a new owner ruling and is deferred. P2 is a smaller fresh-review set, which matters only on table-heavy documents.
4. **Lever** `MODEL_GATEWAY_PATCH_REVISION=off|exact|lenient`, opt-in, default off.
5. **Expected result:** journey 03 follow-up p50 from 44–48 s to about 27–31 s, and p95 from 74–76 s to about 57–62 s. Journeys 02 and 04 are unchanged. Journey 01 changes 0–5 s.
6. **The margin against the ≥15 s p95 criterion is thin.** A second, separate lever for range edits (`from`/`through`, section 1) is the insurance.

## 1. Tool contract

`patch_artifact` is a new builtin in `builtin_tool_defs()` (`tool_loop.rs` ~4210). Its schema is flat, with no `oneOf`:

```json
{"type":"object","properties":{
 "id":{"type":"string"},
 "edits":{"type":"array","minItems":1,"maxItems":12,"items":{"type":"object",
   "properties":{"oldText":{"type":"string","minLength":1},"newText":{"type":"string"}},
   "required":["oldText","newText"],"additionalProperties":false}},
 "title":{"type":"string"}},
 "required":["id","edits"]}
```

**Description, steering the model without touching `author_guidance.rs` (plan wording stays v1):**
- Use it when the user changes one or a few places, such as a named post or section, a sentence, a figure or a date, and the rest must stay exactly as it is.
- `oldText` must be copied character for character from the CURRENT text, including punctuation, non-breaking spaces and dashes.
- `oldText` must occur exactly once. Make it one sentence or one line.
- Every edit applies to the text as it is now, and edits must not overlap.
- To insert, repeat a unique neighbouring line in both `oldText` and `newText`. To delete, send an empty `newText`.
- A failed call changes nothing.
- Use `update_artifact` with the complete content when rewriting most of the document, restructuring, translating or shortening the whole text.

**Companion prompt changes, all applied only when the lever is on:**
- A per-round system message `PATCH_REVISION_NOTICE`, pushed only when the tool is offered. It tells the model to patch for local changes and update for whole-document changes, and says the runtime counts and checks the complete document after applying edits.
- One extra sentence on the `update_artifact` description: "For a change in one or a few places prefer patch_artifact".
- A variant of `ArtifactVersionStore::inventory_context` (`artifacts.rs` 437–457). It currently hard-codes "then update_artifact"; the variant mentions both tools.

**Limits (constants in the new `artifact_patch.rs`):**

| Limit | Value |
|---|---|
| `MAX_PATCH_EDITS` | 12 |
| `MAX_OLD_TEXT_CHARS` | 4,096 (same as the reviewer-edit limit) |
| `MAX_NEW_TEXT_CHARS` | 8,000 |
| `MAX_PATCH_PAYLOAD_CHARS` | 16,000 |
| Result size | existing 200,000-character artifact limit and the 24,000-byte review limit |
| Offered only when | the current text fits what the model saw (see the truncation finding in section 9) |

**When it is offered.** `patch_offered(levers, seen_reads, artifacts, thread_id, new_note_pending)` is true only when all of these hold:
- the lever is not `Off`;
- the thread has an artifact, via the same gate as `tools_for_artifact_state`;
- it is not a new-note round (it is removed together with `update_artifact` at `tool_loop.rs` ~6364);
- the model has seen the artifact's complete current version this turn.

The last condition is tracked by a new `seen_reads: HashMap<id, (version, complete)>`, filled where `sole_revision_source_read` is set (~7096–7117). It covers both the gateway prefetch and the model's own read. After a successful patch the entry is dropped, so a second patch in the same turn requires a fresh read.

**Other name lists to touch:**
- `conversation_tool_allowed` (4151)
- `tools_for_artifact_state` (4192)
- the `builtin_tool_defs()` loop in `sse.rs` ~1640, skipped when the lever is off, following the `reattach_context` pattern
- `turn_timing::round_phase` (`turn_timing.rs` 366) must count `patch_artifact` as Generation. Otherwise `Mark::FirstDraft` and `first_draft_ms` are lost, and the A/B measures nothing.

**Range form (second lever, default off):** `MODEL_GATEWAY_PATCH_RANGE_EDITS` adds optional `from` and `through` to an edit. The edit then replaces the text from the first char of `from` through the last char of `through`. Both must be unique, and `through` must come after `from`. This avoids retyping the old text. It costs about 3 s less streaming per 500 characters replaced, so it goes in arm D of the A/B, not v1.

## 2. Deterministic application

New module `model-gateway/src/artifact_patch.rs`. Types and functions:

```rust
pub struct EditRequest { pub old_text: String, pub new_text: String }   // serde oldText/newText
pub enum MatchMode { Exact, Normalised }
pub struct AppliedEdit { index, before: Range<usize>, after: Range<usize>, mode, old_hash, new_hash }
pub struct Patched { content: String, edits: Vec<AppliedEdit>, base_hash: String, patched_hash: String }
pub enum EditFailure { Empty, NoOp, TooLarge, NotFound{nearest, hint}, Ambiguous{occurrences}, Overlap{a,b} }
pub fn apply_edits(base: &str, edits: &[EditRequest], mode: PatchRevision) -> Result<Patched, Vec<EditFailure>>
pub fn refusal_message(id: &str, version: u32, failures: &[EditFailure]) -> String
pub fn gaps_intact(base: &str, applied: &[AppliedEdit], candidate: &str) -> bool
```

**Algorithm:**
1. Resolve every edit against the SAME base text, never sequentially. Edit 2 cannot match text produced by edit 1.
2. Find occurrences with `match_indices`, plus the overlapping-occurrence check from `apply_review_edits` (`source_validation.rs` 1462–1488), so "aaa" containing "aa" counts as ambiguous. Offsets are byte offsets on char boundaries.
3. Require exactly one occurrence per edit. Sort by start. Reject overlaps; adjacent ranges are allowed.
4. Build the result in one pass: gap, newText, gap, and so on. Never call `replace` on the whole string.
5. Verify `gaps_intact` before returning. This checks that the prefix, each inter-edit gap and the suffix of the result equal the corresponding base slices, byte for byte. Failure is an internal refusal, never a publish.

**Lenient mode.** It applies only after an exact miss, and only on a unique normalised hit. The applied span is the document's own bytes, and the receipt records `matchMode: normalised`. The normalisation set is closed:
- Unicode space separators (NBSP, narrow NBSP and similar) fold to a plain space.
- CRLF folds to LF.
- Curly quotes and «» fold to straight quotes.
- Hyphen-like dashes (–, —, −) fold to `-`.
- `…` folds to `...`.
- Zero-width characters and soft hyphens are dropped.
- Whitespace runs collapse, but newlines are kept.

A per-char offset map handles the expansions. This class matters because Norwegian thousands separators and "21.–25." ranges use NBSP and en dashes.

**Refusal messages are precise and all-or-nothing.** All failing edits are listed in one message, so one retry suffices:
- NotFound: the nearest passage (a bounded window, chosen by token overlap) and, when applicable, "differs only in whitespace/quote/dash at position k".
- Ambiguous: up to 5 occurrences with about 30 characters of context each.
- Each message ends "Nothing was changed. Fix the named edit, or use update_artifact with the complete content."

Model text is never logged; only counts and categories.

## 3. Review mapping

New in `source_validation.rs`:

```rust
pub struct EditScope { spans: Vec<Range<usize>> /*before coordinates*/, pieces: Vec<PieceAnchor>, base_hash: String, reuse_eligible: bool }
impl EditScope { pub fn from_patch(before: &str, patched: &Patched) -> Self;
                 pub(crate) fn resolve_pieces(&self, text: &str) -> Option<Vec<Range<usize>>>; }
pub async fn check_artifact_with_edits(settings, context, request, prompt, before, before_review, content, edit_scope: Option<&EditScope>, infer)
// check_artifact(...) becomes a thin wrapper passing None, so the 60 existing call sites stay untouched.
```

**Change to `revision_carryover` (~897):**
- `InstructionScope::Piece(Range)` becomes `Pieces(Vec<Range>)`.
- A new `revision_scope(prompt, before, edit_scope)` returns the instruction's piece first when a dated post is recognised. Otherwise it returns the patch pieces, when `reuse_eligible`. Otherwise `WholeDocument`, so nothing is reused.
- `in_targeted_piece` becomes "before-range inside any piece".
- The guards are unchanged: same source hash, `CHECKER`, `prior.content_hash == hash(before)` and the same `segment_count`.
- `changed_entities`, `preserved_dependents`, the receipt-bound evidence remap and the `expected.len()==len → None` rule are unchanged.
- `snapshot_carryover` (in-turn recheck) is unchanged.
- `scoped_revision` at ~2448 becomes "instruction piece OR edit scope".

**Pieces.** `EditScope::from_patch` takes each edit span's innermost enclosing heading section, reusing `sections()` from `revision_preservation.rs` and exposing a `pub(crate) enclosing_sections(text, range)`. This handles code fences and bold-label sections the way the existing code does. Pieces are unioned. A preamble before the first heading is its own piece. A document with no headings gets no reuse.

**Ineligible for reuse ("when in doubt review it"):**
- an edit that touches a heading line or a table header or separator row;
- a pure deletion of whole segments;
- more than 4 spans;
- pieces covering more than 50% of the document;
- a fence boundary.

The patch turn then reviews everything, exactly as an unrecognised revision does today.

**The three requested cases:**
1. **An edit spanning a segment boundary.** For example, oldText runs from the end of paragraph A through the blank line to the start of B.
   - The span overlaps two before-segments. Pieces are the sections holding the span's start and end, plus everything between them.
   - The replaced text yields unaligned candidate segments, which are fresh.
   - In-piece aligned siblings are fresh.
   - Segments outside the pieces are carried with their prior evidence, unless they share a changed entity.
   - If the span crosses a heading, the scope is ineligible.
2. **An edit inside a list item or table row.**
   - With `list_item_segments` on, a bullet or row is its own segment, so exactly one segment changed.
   - Under P1 the whole enclosing section is fresh, so a total or aggregate row is covered without depending on `is_aggregate_row`.
   - Neighbouring sections are carried.
   - P2 would narrow this to the row, the aggregate rows and the entity sharers, using the `edited_groups` and `concludes_an_edit` rules. It needs the owner ruling.
3. **An edit that changes a date, amount or name repeated elsewhere.**
   - The `changed_entities` logic is untouched. A preserved segment outside the pieces sharing the entity goes to `expected` and into `preserved_dependents`.
   - The fail-fast repair path calls `targeted_piece(prompt, &candidate)` (~2764), which returns None for a patch and would fail the turn. It must use `EditScope::resolve_pieces(&candidate)` instead. That re-finds each piece by its heading label in the candidate and returns None if the label is missing or duplicated.
   - The repair stays confined to the pieces, as today.

**Extra safety.** A candidate segment whose range overlaps an applied span is never carried, even if `align_segments` matches it by text through a coincidental duplicate. `align_segments` is O(n·m), so a positional gap alignment (n plus the span count) is a valid replacement for the patch path. A property test asserts it is a subset of the LCS alignment.

**Interplay with preservation.** For a recognised DatedPost or InternalNotes instruction, `Preservation::apply` still runs first on the spliced content. Edits outside the piece are reverted, as today, and the receipt counts them (`outOfScopeEditsDiscarded`).

## 4. Failure handling and latency safety

**Where compilation runs.** It runs immediately after `take_tool_phase_signal` and the truncation check, before `rewrites_validated_document`. It skips the truncated call. Function `compile_patch_calls(...)` returns, per call, `Compiled(PatchApplied)` or `Refused(String)`.
- **Compiled:** the call is renamed to `update_artifact` with `{id: resolved, content, title?}`. It flows into the existing check loop, so an unchecked document cannot be published (same gate as today).
- **Refused:** the call keeps its name and skips the check loop. The dispatch closure returns `err_outcome` with the message, the same pattern as the truncated-call error at ~6873. No review call and no audit action are made.

**Extra guards:**
- The check loop re-asserts `hash(before) == patch.base_hash`. A mismatch gives a tool error, not a failed turn.
- Two patches to one artifact in one round: the second is refused with "one patch_artifact call per artifact per round", instead of triggering the `checked_ids` `VALIDATION_FAILED`.
- The progress event stripping at ~6839 must also remove `edits`, because edit text is staged content.

**Cost of a failure.**
- The refusal is detected in microseconds.
- The cost is the wasted round (about 9 s to first delta plus a short output, roughly 10–12 s) plus one retry round of about 10–13 s.
- Worst case for two failures is about 10 + 10 + 32 s of drafting, still below today's failed-attempt cost of +32 s on top of a full draft.

**Escalation.**
- After 2 refusals in a turn, `patch_artifact` is removed from `round_tools` and a one-line system message tells the model to send the complete text with `update_artifact`.
- That is the same-turn full-content fallback.

**A refused patch must not increment `tool_failures`** (~7091). Line 6960 sets `scoped_revision_complete` only when `tool_failures == 0`. Otherwise a retry adds a whole extra `finish_tool_phase` decision round of about 9 s. It also must not increment `document_tool_failures`, which would withhold the deterministic `revision_completion` and add an answer-stream round. Test: `a_refused_patch_followed_by_a_good_one_keeps_the_deterministic_completion`.

**Completion for unrecognised edits.** For DatedPost, the completion is already deterministic. For an unrecognised follow-up, `completes_checked_document` (4602) returns false, so there is one extra finish round. An optional patch-derived arm (`patch_applied && document checks passed && no second-deliverable terms`) fixes that. It has no fixture and is not counted in the projections.

**Restart or replica.** A restart or a replica switch loses the prior review (`rehydrate_persisted_artifacts`, `sse.rs` ~4910, restores text but not reviews). The patch draft saving remains, and the review is complete, as today. The receipt is already persisted in `artifactChecks`. Rehydrating it into `PriorAcceptedReview` is a separate S–M improvement that helps the first follow-up after every redeploy.

**Concurrency.**
- Patches apply to the run's private fork. `lock_commit` plus `unchanged_from(artifact_base)` raise `artifact_revision_conflict` exactly as today.
- There is no auto-rebase in v1.
- The exact-match edits could later be re-applied to the new base after a conflict. Re-review would still be needed.

## 5. Receipts and attestation

`result_checks` (`tool_loop.rs` ~6830) gets an additive `"patch"` object. `SourceReceipt` gets `scope_basis: Option<&'static str>` ("instruction_piece" or "patch_sections"), set from `RevisionCarryover`.

```json
"patch": {"schemaVersion":1,"tool":"patch_artifact","matchMode":"exact",
 "baseVersion":2,"baseHash":"<blake3 of before>",
 "edits":[{"index":0,"beforeRange":[a,b],"afterRange":[c,d],"oldHash":"..","newHash":".."}],
 "patchedHash":"<hash right after application>","resultHash":"<hash of the checked, committed content>",
 "modifiedAfterApply":false,
 "outsideEditsIdentical":true,
 "bytes":{"removed":n,"added":m,"unchanged":k},
 "changedSegments":[..],"freshReviewed":[..],"carried":[..],
 "outOfScopeEditsDiscarded":0}
```

- **`outsideEditsIdentical`** is recomputed by the gateway on the final content, not the model's claim. It means every base gap appears verbatim and in order in the result.
- **`modifiedAfterApply`** is true when preservation, a reviewer edit or a repair changed the spliced text.
- **Replay proof.** Anyone can replay `apply_edits(base_v, edits)` and get `patchedHash`.
- **Strictly stronger than today.** Today's `artifact_revision_binding` proves only previous and new hashes. This adds what changed, where, and what was carried versus re-reviewed. `reusedFromReceipt` and `reusedSegments` are unchanged.

## 6. Frontend

No change is needed. The wire contract still carries full content per version. The version diff (`diffText`, `DiffView`) and the reload equality assertions still work.

- The Arbeid step list already renders `StepUpdate`. The gateway emits `"{call.id}:patch"` with Norwegian text ("Målrettede endringer", "2 endringer anvendt, resten er uendret"). No FE code is needed.
- Optional S-effort follow-up: a small "N endringer" chip in `ChatArtifactPanel.tsx`. It would need an additive `artifact.revision` field on the wire, so it is out of v1.

## 7. Lever and A/B

**Lever.**
- `LatencyLevers` gets `patch_revision: PatchRevision` (`Off`|`Exact`|`Lenient`, `Default = Off`). The struct stays `Copy`.
- `ALL_ON` and `ALL_OFF` both carry `Off`. `parse()` takes a 10th argument, and unrecognised values mean Off.
- New `ALL_ON_PATCH` for tests.
- This departs from the on-unless-off pattern of the other levers. It is deliberate: the change is model-visible, and existing tests pin the advertised set with `ALL_ON`.
- `docker-compose.yml` (model-gateway env, next to `MODEL_GATEWAY_LIST_ITEM_SEGMENTS`, ~292): `MODEL_GATEWAY_PATCH_REVISION: ${MODEL_GATEWAY_PATCH_REVISION:-off}`. The range lever gets the same treatment.
- The launcher's Control Plane env chain must export it. Per the memory rule, build via the launcher, not a hand-built compose.
- The startup `tracing::info!("latency levers")` gains the new field.

**Prerequisite instrumentation (S).**
- Log draft `input_tokens` and `output_tokens` at ~6557–6566, content-free.
- Add `patch_outcome`, `patch_edits`, `patch_match_mode`, `patch_failures` and `patch_output_chars` to the `turn timing` record.

**Arms.** All run on gpt-5.6-terra via openai-codex-subscription (never Balance), interleaved per take on the same build and day:
- A: off
- B: `exact`
- C: `lenient`, only if whitespace-class misses are at least 30% of B's refusals
- D: `exact` plus range edits

**Journeys.**
- **03 follow-up:** n=20 per arm, then n≥30 for the winning arm.
- **02 and 04 follow-ups:** n=20 for A and B. Both are new-note turns, so `patch_artifact` must never be offered (`tools` in the timing record). They act as no-regression controls.
- **01:** n=10 for A and B.
- **Extra 03 variants,** added to the 03 journey's REGI.md and selected by a `VEREVON_PATCH_ARM` env in `tests/e2e/product-scenarios.spec.ts`, n=10 each on B:
  - insert a sentence into a post;
  - change a date repeated elsewhere;
  - a whole-document shortening, where the model should choose `update_artifact`.

**Saved matrix (run by the orchestrator, not implementers).** Implementers deliver:
- `tests/fixtures/patch_scope/*.json`, at least 12 labelled cases;
- new rows in `tests/source_recheck_live.rs`;
- `VEREVON_REVIEW_EVAL_PATCH_SCOPE=1`, which routes revision rows through `check_artifact_with_edits`.

**Success criteria (all must hold):**
1. 03 follow-up p95 ≤ baseline p95 − 15 s and ≤ 70 s; median at least 12 s lower.
2. No rise in rejections: the share of takes with `review_passes>1`, `repairs>0` or `reviewer_edits>0` is at most A + 5 percentage points (with n=20, at most +1 take).
3. Zero oracle failures above baseline:
   - the e2e byte-identity assertions for the 21 and 25 September posts and the email;
   - the 4 document checks;
   - reload equality;
   - no unsupported claim published.
4. Patch health:
   - first-try application ≥ 90%;
   - fallback to `update_artifact` ≤ 5%;
   - adoption ≥ 90% on local-change follow-ups;
   - a source-review receipt on every published patched document.
5. The saved matrix stays at 102–104/104 with the lever on, and the new patch-scope rows pass 100%.

Kill criteria: first-try application under 80%, or any unchecked document published. Either keeps the lever off.

## 8. Expected savings, effort, risks

**Draft model.** A 03 follow-up draft is about 8.8 s to first delta plus 6 ms per output character. A patch with exact old and new text is about 1.0–1.2 KB, so the draft is about 13–19 s (central 15 s), against the measured 32.5 s. With range edits it is about 12.5 s.

| Journey | Follow-up type | Expected change |
|---|---|---|
| 03 | scoped edit of a whole document | draft −13 to −21 s. Median 44–48 s → about 27–31 s. p95 74–76 s → about 57–62 s. Review is unchanged. |
| 01 | rewrite of the whole customer body | 0 to −5 s. Whole-body tone changes should choose `update_artifact`. |
| 02 | new note | 0 |
| 04 | new note | 0 |

Per-take projection for the 10 q16f 03 takes with a 16 s draft: 25.5, 55.9, 37.4, 23.2, 26.9, 31.0, 43.0, 25.3, 29.5 and 45.7 s. The retry take falls from 75.6 to 43.0 s.

One caveat: the two q16e review-loop tails (76 s and 91 s) stay near or above 70 s, roughly 1 in 30. The p95 target holds. The ≥15 s p95 criterion is met with little margin, which is why arm D matters.

**Effort.**

| Service | Work | Size |
|---|---|---|
| model-gateway | `artifact_patch.rs` plus tests | M |
| model-gateway | `tool_loop.rs` compile, gating, notices, refusal routing, `tool_failures` handling, timing | M |
| model-gateway | `source_validation.rs` `EditScope`, `Pieces` and the fail-fast resolver | M–L |
| model-gateway | `latency_levers.rs`, `turn_timing.rs`, `artifacts.rs` | S |
| model-gateway total | about 8–10 engineer-days | L |
| deploy compose and launcher env | | S |
| live fixtures and spec arm switch | | S–M |
| frontend | none required; optional chip | S |
| inference-core, execution-core, Control Plane | none | – |

**Tests (names).**
- `artifact_patch.rs`:
  - `exact_edit_replaces_one_unique_occurrence_and_leaves_every_other_byte`
  - `an_old_text_found_twice_names_every_occurrence_and_changes_nothing`
  - `an_old_text_found_zero_times_offers_the_nearest_passage`
  - `overlapping_or_identical_edits_are_refused_as_a_set`
  - `edits_apply_against_the_base_not_one_after_another`
  - `overlapping_occurrences_count_as_ambiguous`
  - `multibyte_norwegian_text_keeps_char_boundaries`
  - `normalised_match_needs_a_unique_hit_and_replaces_the_documents_own_bytes`
  - `size_limits_hold_per_edit_and_in_total`
  - `applied_gaps_are_verified_byte_for_byte` (seeded random property test)
  - `edit_scope_pieces_match_the_dated_post_piece_for_a_post_edit`
  - `heading_or_table_header_edit_makes_scope_ineligible`
- `source_validation.rs`:
  - `patch_scope_carries_sections_outside_the_edited_one`
  - `patch_scope_edit_in_list_item_reviews_the_whole_section_only`
  - `patch_scope_boundary_spanning_edit_reviews_both_sections`
  - `patch_scope_changed_date_forces_dependents_outside_the_piece`
  - `patch_scope_rejected_preserved_dependent_gets_one_confined_repair`
  - `patch_scope_is_ignored_when_prior_receipt_does_not_match`
  - `patch_scope_never_carries_a_segment_overlapping_an_edit`
- `tool_loop.rs`:
  - `patch_artifact_is_offered_only_with_the_lever_and_a_full_read_this_turn`
  - `new_note_turns_never_offer_patch_artifact`
  - `a_valid_patch_dispatches_as_update_artifact_with_the_spliced_content`
  - `a_failed_patch_returns_one_precise_error_and_dispatches_nothing`
  - `a_second_failed_patch_withdraws_the_tool_for_the_rest_of_the_turn`
  - `two_patches_to_one_artifact_in_a_round_error_instead_of_failing_the_turn`
  - `a_patch_progress_event_never_carries_edit_text`
  - `turn_timing_counts_patch_artifact_as_generation`
  - `patch_lever_off_sends_byte_identical_requests`
- `latency_levers.rs`: `patch_revision_is_off_unless_opted_in`.
- `tests/e2e_invoke_chain_test.rs`, following the existing `run_note_thread_turn` and `write_round` helpers with a new `patch_round`:
  - `a_scoped_revision_by_patch_reuses_the_previous_review_and_publishes_the_spliced_document`
  - `a_patch_that_changes_a_date_repeated_elsewhere_reviews_the_dependent_segment`
  - `a_bad_patch_costs_one_round_then_publishes_via_update_artifact`
  - `a_patched_document_that_fails_source_review_is_never_published`
  - `patch_receipt_records_base_and_result_hashes`

**Risks.**
1. **Terra's exact quoting.**
   - Multi-line JSON-escaped strings are the weak point.
   - Steering: one line or sentence per `oldText`.
   - The lenient mode covers whitespace, quote and dash slips.
   - Kill switch at under 80% first-try success.
2. **Norwegian typography and NBSP.**
   - NBSP in "3 000 kr", en dashes in ranges, and «» versus quotes.
   - The lenient mode's closed normalisation list covers these.
   - Newly typed plain spaces in a `newText` still change bytes, but only inside the fresh-reviewed edit.
   - Track a `nbsp_lost_in_new_text` counter.
3. **Markdown and encoding.**
   - The store keeps exact bytes.
   - The model sees a `.trim()`med read.
   - CRLF documents miss in exact mode.
   - Combining diacritics (NFC versus NFD) are not normalised.
4. **Adoption and output size.** The model might patch a whole rewrite, or rewrite in full when a patch was better. The receipt's `wholesale` flag and adoption metric catch this.
5. **Review cost is unchanged.** Heavy batches (1 in 10 takes reach 55 s or more), rejection loops and the roughly 9 s per-call floor are untouched.
6. **Sections and repairs.**
   - Nested headings or headingless documents give no reuse; the draft saving remains.
   - Repairs after a rejected preserved dependent need the new resolver.
7. **Concurrency:** unchanged (section 4).
8. **Measurement:** `round_phase` must be updated, or the A/B is invalid.
9. **Prompt-cache prefix** changes for patch arms only.

## 9. Anchors verified in code

- **Read truncation (new).**
  - `bounded_tool_output` (`tool_loop.rs` 4465) cuts any tool result at `MAX_TOOL_OUTPUT_CHARS = 8000` (line 186), although `read_artifact` reads up to 24,000 and the prefetch fires up to 24,000.
  - The model therefore sees at most about 7.9k characters of the document.
  - The patch gate needs the text fully visible.
  - This is also an existing latent gap for whole-document rewrites of documents over 8k.
- **Injection point.** The check loop starts at ~6684 and skips a document with no source context and no preservation (~6689–6695). Compilation must run before the loop for all patch calls.
- **`tool_failures == 0` at ~6960** gates `scoped_revision_complete` (section 4).
- **`turn_timing.rs` 366** `round_phase` (section 1).
- **`source_validation.rs` ~2764** fail-fast path (section 3).
- **`artifacts.rs` 455** hard-coded "then update_artifact" (section 1).