# Revision-turn map for the follow-up latency design

Read-only. The only file I wrote is a scratch `reqids.json` in my session scratchpad, outside the repo.

Paths: MG = `C:/dev/CoresSystem/apps/Model Plane/rust/services/model-gateway/src`, FE = `C:/dev/CoresSystem/apps/Frontend Plane/verevonv3`. Timing data is the Playwright attachments (`revision-stream`, `revision-phases`) in `C:/private/verevon-recordings/q16f-20260928T1814Z` (03 n=10, 04 n=9) and `q16e-20260928T0202Z` (03 n=20, 04 n=18). The gateway log is `gateway_q16f.log` in the scratchpad.

## Summary

- Only journey 03's follow-up is a scoped edit of an existing document. It rewrites about 3.8 KB to change about 0.43 KB (11%).
- Journeys 02 and 04 create a new short note, and journey 01 rewrites its whole customer body. A patch cannot help those.
- 03 follow-up draft time is about 9 s fixed plus about 6 ms per output character. A patch-sized draft would be about 13 s instead of about 32 s.
- Pooled over q16e and q16f (n=30), 03 follow-ups are p50 43.9 s and p95 76.0 s. A patch-sized draft projects to about p50 24 s and p95 57 s. Review tails would still remain.

## 1. Tools and what the model emits on a follow-up

**Tool definitions** (`MG/tool_loop.rs`):
- `create_artifact` (4247-4251) takes id, kind, title and content.
- `read_artifact` (4262-4266) takes an optional id. With one artifact and no id it reads that artifact (2742-2744).
- `update_artifact` (4267-4271) takes id, optional title and `content`, described as "the complete replacement content". Its description says "Always send the COMPLETE new content, not a diff or a fragment".
- `RESPONSE_DISCIPLINE_NOTICE` (4132-4142) says to revise with `update_artifact` under the same id. `tool_decision_messages` (4803-4811) asks for "full content in artifact create/update arguments".
- A rewrite goes through `create_artifact` too, so the whole document is emitted either way. Its dispatch redirects a known id or title to the existing artifact (2566-2578).

**Which tools are offered:**
- The client's tools plus the built-ins are offered when the "tools" feature is on (`MG/sse.rs:1622-1662`).
- A conversation-only source scope keeps only create/read/update_artifact, count_words, code_interpreter and reattach_context (`sse.rs:1671-1672`, `tool_loop.rs:4151-4153`).
- Per round (`tool_loop.rs:6357-6386`):
  - read and update appear only once the thread has an artifact (4192-4195).
  - count_words is removed when the campaign contract or word-count guidance applies (6359).
  - A new sourced note removes `update_artifact` (6364-6366).
  - After a full read of the sole artifact, `read_artifact` is removed (6367-6373).
  - `finish_tool_phase` is added unless a checked document is still pending (4813-4820, 6386).
- On a 03 follow-up the model therefore sees roughly create_artifact, update_artifact, code_interpreter and finish_tool_phase.
- The decision request uses `thinking_budget_tokens: 0`, `max_tokens: 8192` (`TOOL_ROUND_TOKENS`, 147), `tool_choice: required` and `cache-control: no-store` (6435-6465).
- A cut-off write is refused with a message telling the model to write a first section and extend it (6636-6650, 6874-6881).
- The content limit is 200,000 characters (`MG/artifacts.rs:92`). Review refuses a candidate over 24,000 bytes or more than 100 segments (`source_validation.rs:2195`).

**How ids resolve:**
- `update_artifact` dispatch (2605-2641) tries `resolve_similar_id`, then `id_for_title(title_arg)` if the title is non-empty, then the raw id.
- An unknown id is refused, and the error lists the artifacts that exist (2645-2670).
- The check loop resolves ids separately (6690-6694).
- `resolve_similar_id` (`artifacts.rs:398-417`) matches an exact known id first, otherwise a unique match on lower-cased letters and digits. `id_for_title` is a case- and whitespace-insensitive index (387-391).
- The gateway assigns versions (`next_version`, `artifacts.rs:219-224`). The kind is immutable (`tool_loop.rs:2579-2581`).

**Prefetch and narrowing:**
- `sole_artifact_prefetch` (4871-4903) is gated by the `sole_note_prefetch` lever and needs all of:
  - `read_artifact` offered.
  - exactly one known artifact in the thread.
  - that artifact at most 24,000 characters (4147).
  - either a pending new note, or `Preservation::from_prompt` returning `Ok(Some)`.
- It runs once, before the first model round (6410-6424). It is a synthetic tool call with id `gateway-sole-artifact-prefetch-read` (4845), dispatched through the audited path, and framed by `format_gateway_read_context` (4379-4385, 7151-7162).
- Measured cost is 17-38 ms.
- A successful whole read sets `sole_revision_source_read` (7111-7117) or `sole_note_source_read` (7096-7102).
- `Preservation::from_prompt` (`MG/revision_preservation.rs:116-193`) recognises only two shapes:
  - `InternalNotes`: "behold/preserve/keep" plus one of four note phrases.
  - `DatedPost`: the instruction must start with "gjør/endre innlegget for " or "revise/make the post for ", then `D. Month`. A mixed-request clause list (about 10 phrases, such as "og oppdater") is excluded.
- Any other follow-up is not prefetched. The model must call `read_artifact` itself, which costs an extra model round.

## 2. From new content to a version and a review

**Version creation** (`MG/sse.rs`):
- The turn works on a private fork: `committed_artifacts`, then `artifact_base = fork`, then `session_state.artifact_versions = fork of base`. `artifact_review_base = artifact_base` is frozen at turn start (1783-1794, `state.rs:265-292`).
- `update_artifact` dispatch writes `next_version` and `remember` into that fork (`tool_loop.rs:2702-2706`). `tool_artifact_events` (334-417) harvests the artifact event, and it is emitted at 7084-7086.
- Commit (`sse.rs`):
  - takes `lock_commit` (3374)
  - fails with `artifact_revision_conflict` if `unchanged_from(artifact_base)` is false (3376-3392)
  - saves the assistant message durably (metadata `artifacts` holds the latest full content per id, `session_flow.rs:1500-1503`)
  - then calls `seed_persisted`, `remember_kind` and `adopt_review` (`sse.rs:3665-3679`, `artifacts.rs:375-383`).
- `adopt_review` copies the run's review only if the committed version and content hash match it.
- Restart hazard: `rehydrate_persisted_artifacts` (`sse.rs:4910-4954`) restores content, kind and version but not reviews. After a gateway restart, or on another replica, the first revision has no prior review and reviews everything. The stores are process-local (`artifacts.rs:136-152`).

**Review consumption** (`tool_loop.rs:6678-6828`):
- For each document write, `before` is the fork's current content. The gateway looks up `prior_review` in `artifact_review_base` only (6741-6742), then calls `check_artifact` (`source_validation.rs:2410-3000`). This runs only if source context exists or a preservation shape matched (6704).
- `check_artifact` runs these steps:
  1. **Preservation first.** `Preservation::apply` runs at the start of every attempt (2508-2516). For `DatedPost` it splices the model's target section into `before`, so everything outside that section is `before` byte for byte. The model's output outside the target is discarded. `InternalNotes` does the same for the notes section (`revision_preservation.rs:205-232`).
  2. **Local checks.** Word limits (2518-2543), the campaign contract (2544-2567) and the calendar check (2568-2586).
  3. **Carry-over.**
     - `revision_carryover` (`source_validation.rs:897-993`) needs: same source hash, checker `attachment-source-review-v16`, `prior.content_hash == hash(before)`, and the same segment count (905-911).
     - It returns `None` for a whole-document scope (912-915), so an unrecognised instruction reviews every segment fresh.
     - For a targeted piece, segments are aligned by byte-identical text (`align_segments`, 474-500). Segments inside the piece, and preserved segments that share a changed date, amount or name (`changed_entities`, 745-782), go to review. The rest are carried with their evidence.
     - `snapshot_carryover` (810-895) does the same narrowing for in-turn rechecks.
  4. **Review call.** `review_candidate` (2162-2352) sends the full sources plus all segments on every call (2324-2331). The reviewer is forced to return one verdict per assigned index (2241-2258). Assigned sets of 3 or more are split into batches (`REUSED_REVIEW_BATCH_MIN`, 1390; `batched`, 2337-2339). The subscription route uses `thinking_budget_tokens: 4096` and `max_tokens: 6000` (2272, 2281).
  5. **Repair loop.** Reviewer edits (2815-2820) and targeted repairs (2823-2997). A rejected preserved segment gets one confined repair, then fails fast (2746-2782).
- The receipt (`SourceReceipt`, 1132-1194) records `reviewed_segments`, `reused_segments`, `reused_from_receipt`, `carried_in_turn_segments`, review tokens and `validation_ms`.
- The accepted content replaces `arguments_json.content` (`tool_loop.rs:6821-6822`), and the remembered review is stored at 6802-6805. The dispatch and completion below then run on that string.
- `completes_checked_document` (4572-4607) ends the tool phase without another model round only for `DatedPost` with 4 document checks, or `InternalNotes` with the length check passed. An unrecognised revision costs a further decision round.
- There is no final model call after the tool phase. `revision_completion` produces a fixed Norwegian summary and the `artifact_revision_binding` receipt (`result_validation.rs:426-491`, `sse.rs:2358-2415`).

## 3. Frontend

- The wire contract is `artifact {id, kind, title, content, version}` with full content on every version (`FE/src/features/chat/components/chat-artifacts.ts:1-13`).
- The client keeps every version's full content in `history` (`mergeArtifactVersion`, 478-495; `upsertArtifact`, `chat-normalizers.ts:1599-1609`; `onArtifact` in `use-chat-controller.ts` around 3013-3031).
- `ChatArtifactPanel.tsx` shows a version stepper (310-330) and a "Vis endringer" toggle (333-346). The diff is computed in the browser by `diffText(previous.content, revision.content)` (202-206), rendered by `DiffView` (413). That is a line-level LCS capped at `MAX_DIFF_LINES = 2000` (`FE/src/shared/chat-nodes/diff.ts:61`).
- The FE never reads `reusedSegments` or `reviewedSegments` (grep found nothing).
- The e2e spec asserts:
  - after reload, id, version and content equal the streamed ones (`tests/e2e/product-scenarios.spec.ts:279-288`)
  - for 03, the 21 and 25 September posts and the email are unchanged (253-257)
  - the revision receipt has 4 document checks (250).
- If the gateway compiles any edit to a full document before publishing, the FE needs no change.

## 4. Where follow-up wall time goes

### 03 follow-ups (q16f, seconds)

| take | total | draft | review | note |
|---|---|---|---|---|
| 1 | 39.6 | 30.1 | 9.2 | |
| 2 | 72.0 | 32.1 | 39.5 | heavy batch: 3,008 output tokens |
| 3 | 56.2 | 34.8 | 21.2 | 1,432 output tokens |
| 4 | 42.1 | 34.9 | 7.0 | |
| 5 | 42.7 | 31.8 | 10.7 | |
| 6 | 47.5 | 32.5 | 14.8 | |
| 7 | 75.6 | 64.6 | 10.8 | draft attempt 1 failed after about 32 s, retried in full (`attempts: 2`, handler 64.6 s vs provider 32.1 s) |
| 8 | 39.2 | 29.9 | 9.1 | |
| 9 | 49.4 | 35.9 | 13.3 | |
| 10 | 62.2 | 32.5 | 29.5 | 2,143 output tokens |

- p50 48.4 s, p95 74.0 s.
- Draft is 68% of total on average and review 31%. Everything else (prefetch, dispatch, commit, publish) is 0.1-0.3 s per turn.
- Draft median is 32.5 s: first delta 8.8 s (range 7.2-11.4), then 23.1 s of streaming.
- Streaming is 5.9-6.5 ms per output character (median 6.0) for documents of 3.4-4.2 KB.
- Tokens are not recorded for the draft:
  - `tokens_reported` covers only the answer call (`turn_timing.rs:326-329`).
  - The usage event shows 0 on this route.
  - `resp.output_tokens` is logged only when no tool call is returned (`tool_loop.rs:6605-6613`).
  - Review batches do log tokens.
- Estimate: about 1,000-1,300 output tokens per draft (assumes 3.3-3.6 characters per token).
- Changed span is a mean 428 characters of a mean 3,796 (11.3%). The whole target section (heading, status line, body) is 480-800 characters.

### 03 review

- Review is about 8-9 s per batch even for a 1-segment batch, because each call resends the roughly 10k-token full context (input 9.7-10.6k tokens per batch in the log).
- Heavy batches carry 2,000-3,000 output tokens and take 29-40 s. Segments were balanced by bytes (about 1 KB per batch) but the token output was very skewed (3,008 vs 283, 2,143 vs 274).
- Reviewed segments per take were 3-15 (median 6), of 28-51 total. Reused were 21-36.

### 04 follow-ups (q16f, n=9)

- Totals were 19.8-47.0 s, p50 30.4, p95 42.8.
- Draft is 8.3-15.7 s. Review is 10.6-36.9 s.
- The three slowest each had 2 review passes and 1 reviewer edit.
- Review is 51-63% of the turn (draft 8-16 s, review 11-37 s).

### 01 and 02 follow-ups (q16b to q16e, 2-3 recording roots each)

- 01 p50 28-36 s, max 42-59 s (draft about 15 s, review 12-16 s).
- 02 p50 24 s, max 40-52 s (draft about 11 s, review 12 s).

### Tail causes, 03 pooled n=30

- 4 of 30 exceed 70 s.
- Causes seen: a full retry of a failed draft (1), a heavy single review batch, and two rejection, repair and recheck loops in q16e (76.3 s and 90.9 s, review 47.6 s and 58.9 s, 2 passes and 1 repair each).
- A patch removes only the draft share, and halves the retry loss.

### Projection, 03 pooled n=30

Replacing the draft with about 13 s (8.8 s fixed plus about 700 characters at 6 ms, doubled if retried) gives p50 about 24 s, p95 about 57 s and max about 72 s, with 1 turn still over 70 s. For contrast, the initial 03 turn's full-document review has a wall time of about 28 s at p50 (4 batches). That is what an unrecognised whole-document follow-up would pay today.

## 5. Follow-up prompts and where a patch helps

Prompts come from each journey's `REGI.md` (line 12-13; the spec reads them at `product-scenarios.spec.ts:225`) under `FE/apps/verevon-web/plans/product-recordings/`.

| Journey | Follow-up prompt | Today | Patch fit |
|---|---|---|---|
| 01 | "Gjør svaret litt varmere og kortere, maks 100 ord. Behold usikkerheten rundt leveringsdatoen og den interne kildeoversikten." | Recognised as `InternalNotes`, so prefetch fires and `update_artifact` rewrites about 1.2 KB. The notes section (mean 46% of the document) is copied from `before` anyway. | No. The whole body is new. A section-scoped edit might save about 3 s of streaming. |
| 02 | "Kok dette ned til et ledernotat på maks 120 ord, med de tre viktigste tallene og første anbefalte oppfølging." | A new sourced note. `update_artifact` is removed and `create_artifact` writes about 0.6 KB. | No. Output is already tiny and it is a new artifact. |
| 03 | "Gjør innlegget for 23. september mer konkret for et kontor som deles av flere team. Behold den rolige tonen og bruk bare dokumenterte produktegenskaper." | `DatedPost`, so prefetch fires and the model emits the full 3.4-4.2 KB. | Yes. This is the case that pays. |
| 04 | "Lag en kort intern status til prosjektleder med de to viktigste risikoene og beslutningen som må tas først. Maks 100 ord. Ikke send den." | A new note (`create_artifact`, 300-600 characters). The plan stays current. | No. Draft is 8-16 s; review dominates. |

The two other edit types named in the brief do not occur in any fixture, so they have no timing data here:
- **"Add a status note" as a new section in the same document:** a pure insertion, an ideal patch. It is unrecognised today, so prefetch and cross-turn reuse are off.
- **"Make the tone shorter" across the whole document:** every piece changes. A patch saves only the unchanged parts (Kilder, overview, roughly a third of the document). Fresh review of all changed segments stays as required.

## 6. Constraints for the design

1. **Injection point:**
   - Compile a patch call into an `update_artifact` call with the full spliced `content`, inside the loop at `tool_loop.rs:6687-6706`. `check_artifact`, `remember_review`, dispatch, events, commit, `adopt_review` and the FE stay unchanged.
   - The splice from `before` guarantees byte identity outside the edit.
   - Add a bad-anchor error path back to the model.
   - The dispatch guard at 6867 compares `call.name` against the offered tools, so a renamed call must still be offered.
2. **Places that list tool names** and need updating for a new tool: `builtin_tool_defs` (4210), `conversation_tool_allowed` (4151), `writes_document` (3846), `artifact_id_of` (313), `tool_artifact_events` (345), `stops_at_validation_deadline` (3853), and the tests that pin the advertised set (about 8571, 8645-8668, 8740-8749, 10159-10212).
3. **Prompts to adjust:**
   - The `RESPONSE_DISCIPLINE_NOTICE` line at 4142.
   - `CampaignContract::guidance` (`document_contract.rs:205`), pushed every round at `tool_loop.rs:6243-6245`. It steers the model toward writing a complete document.
   - `automatic_word_count_guidance` (4791-4792).
   - `author_guidance.rs` does not mention the artifact tools, so plan-wording v1 is untouched.
4. **Owner ruling needed:**
   - Today `revision_carryover` reuses approvals only for a recognised dated-post scope (`instruction_scope`, 704-709, and the gate at 912-915).
   - A patch knows its changed spans by construction, so a patch-derived scope could give byte-identical, receipt-bound reuse for any edit.
   - That is a policy change beyond the 2026-09-27 dated-post ruling. Changed and dependent segments would still be reviewed fresh, and the review would still see the complete document.
   - The scoped paths (`scoped_revision` at 2448 and the fail-fast repair at 2764) resolve the piece from the instruction in candidate coordinates. A patch-derived piece would need an equivalent resolver.
5. **Prefetch:** to patch an unrecognised follow-up the model must see the text. Widening the prefetch gate at 4890-4897 (sole artifact, at most 24k characters) saves a model round, but it changes prompts for non-edit turns too. `completes_checked_document` (4602-4606) would also need a patch-derived completion, or the tool phase costs one more `finish_tool_phase` round.
6. **Not helped by a patch:**
   - Heavy review batches.
   - Rejection, repair and recheck loops.
   - The roughly 8-9 s per-batch review floor.
   - Loss of reuse after a restart or on another replica.
7. **Instrumentation gap:** logging draft `input_tokens` and `output_tokens` at the generation span (near `tool_loop.rs:6557-6566`) would replace the character-based estimate.