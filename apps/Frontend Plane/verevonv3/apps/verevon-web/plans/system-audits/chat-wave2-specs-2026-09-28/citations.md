# Implementation spec: claim-bound inline citation chips (KB + web, grouped by domain)

Repo root `C:/dev/CoresSystem`, verified as a real git checkout (`.git` present, `git log` shows real history, `git status` shows the same in-flight frontend/gateway files as `apps/CODEBASE_INFORMATION_SYSTEM.md` lists). All paths below are relative to this root unless given in full.

Wave-2 item: **inline citation chips at claim boundaries** (from `apps/Frontend Plane/verevonv3/apps/verevon-web/plans/system-audits/CHAT_PAGE_GAP_PLAN_2026-09-28.md:84,161`).

---

## 1. What exists today (file:line evidence)

### 1.1 Client is fully wired and waiting for data

- `Citation` type already reserves the fields: `apps/Frontend Plane/verevonv3/src/features/chat/components/chat-types.ts:89-100` — `claimId?`, `sourceGroupId?`, `start?`, `end?`, with a comment: *"the current gateway may omit it; the UI must never infer these values from prose."*
- The wire parser already reads them off the SSE payload, both cases: `src/shared/api/chat-client.ts:838-851` (`emitCitation`) reads `payload.claim_id ?? payload.claimId`, `payload.source_group_id ?? payload.sourceGroupId`, `payload.start ?? payload.start_offset ?? payload.startOffset`, same for `end`. This is dead code today — every producer omits these keys.
- `normalizeCitation` forwards them 1:1 when present: `src/features/chat/components/chat-normalizers.ts:1295-1303`.
- `'citations'` is in `DEFAULT_FEATURES` (`chat-client.ts:793`), and `citation` / `citations` / `search_results` SSE event names all dispatch to the same handler (`chat-client.ts:1415-1427`), so citations stream by default on every turn already.
- `use-chat-controller.ts:2960-2969` `onCitation` → `addAssistantCitation` (`:2895-2909`) → `upsertCitation` (`chat-normalizers.ts:1613-1616`, dedups by `id`/`url`, **first-write-wins, later duplicates are silently dropped** — important for §2) → appended onto `turn.citations`, in arrival order.
- Render path: `ChatMessages.tsx:481` passes `props.message.citations` (i.e. `turn.citations`, scoped per turn) down through `ChatMarkdown`/`MarkdownBlockView`/etc. into `parseInline(text, citations)` at every leaf (`ChatMessages.tsx:1040,1054,1068,1071,1369,1378,1439,1470`).
- `parseInline` (`src/features/chat/components/chat-media-markdown.tsx:1132-1201`): a single regex tokenizes code/bold/italic/math/markdown-links/citation-markers/bare-URLs. The citation branch (`:1170-1184`) matches `[n]` or `[n, m]`, resolves each index **positionally** as `citations[index-1]`, and only promotes to a chip if *every* requested index resolves — otherwise the marker is left as literal text.
- `InlineCitationMarker` (`:1055-1086`) renders a `<details>` chip: label = `hostname(first.url)` + `+N`; popover lists every matched citation in one flat list (title, snippet, link). **No domain/provider grouping exists** — two citations from the same domain each get their own row.
- The doc comment right above it (`:1047-1054`) says explicitly: *"the server-issued claim-anchor contract described in the chat plan can replace this lightweight marker path later without changing the source-card surface"* — this is the exact gap.
- Sources/"Kilder" tab: `collectEvidenceSources` (`chat-media-markdown.tsx:146-176`) merges `turn.grounding.sources` (kind `'knowledge'`) and `turn.citations` (kind `'web'`) across **all** turns into one list, keyed by `documentId`/`url`. `partitionEvidenceSources` (`:113-124`) only splits read/unread — no domain grouping there either. Consumed by `SourcesPanel` (`ChatPanels.tsx:353-355`).
- CSS: `.verevon-chat-citation-chip*` in `src/styles/global.css:25556-25660`.
- Existing tests to extend: `chat-markdown.test.ts:167-183` (`parseInline` citation-marker promotion), `chat-normalizers.test.ts:59-81` (`normalizeCitation`), `chat-client.test.ts:594-615` (`emitCitation` wire mapping, currently asserts **no** claim/anchor keys are ever populated).

### 1.2 Server: three independent, uncoordinated citation producers, zero claim binding

`ChatEvent::Citation` (`apps/Model Plane/rust/services/model-gateway/src/sse_events.rs:48-54`) carries only `{id, title, url, snippet}`. `payload()` (`:270-277`) serializes exactly those four keys. It shares the always-default-on `"citations"` feature family with `Grounding` and `Verification` (`:183-185`).

Three producers create `ChatEvent::Citation`, **each numbering its own `[n]` independently, with no shared counter**:

1. **KB/RAG grounding** — `retrieval.rs`. `wants_grounding` (`:80-82`) gates it; resolved unconditionally early in the request (`sse.rs:1333`), **before** deep research or web_search are decided (`sse.rs:1560-1625,1858`) — i.e. KB grounding and web/deep-research citations **can legitimately co-occur in one turn**.
   - `build_context_block` instructs the model: *"cite sources by their bracketed number (e.g. [1])"* (`retrieval.rs:626-630`).
   - `add_context_entry` (`:422-469`) numbers every context **entry** (`entries.len()+1`), citable or not, but only pushes a `GroundingCitation` for entries clearing `MIN_CITABLE_CONTENT_CHARS` (150 chars, `:451,462-469`). **Bug**: a non-citable entry still consumes a `[n]` slot in the prompt, so a later citable entry's prompt-visible number can be **ahead of** its true position in the `citations` vec. Example: entry 1 non-citable (no `GroundingCitation` pushed), entry 2 citable → pushed as `citations[0]` but labeled `[2]` in the prompt. If the model dutifully writes `[2]`, the client's `citations[2-1] = citations[1]` resolves to the **wrong source** (or nothing, if only one citation exists).
   - Citations are emitted "up front" as `ChatEvent::Citation` frames before the answer streams (`sse.rs:1794-1818`), from `grounding.citations: Vec<GroundingCitation>` (`retrieval.rs:178`).
   - `GroundingSource` (kind `'knowledge'`, `retrieval.rs:95-105,582-607`) already carries `provider` (normalized via `source_label`, `:310-339`, e.g. `"Microsoft 365"`, `"Google Workspace"`, `"Notion"`) — but `GroundingCitation` (`:86-91`) does **not** carry provider today.
2. **Ad-hoc `web_search` tool citations** — `tool_loop.rs`. `web_search_citations` (`:5409-5426`) builds one `ChatEvent::Citation` per kept/citable hit, `id: format!("web-{}-{}", rank+1, hit.url)`. The model-facing tool result independently numbers `KEPT` hits starting at `1.` again (`gated_web_search_output`, `:5511-5525`), with **no awareness of how many `[n]` slots KB grounding's system-context block already used in the same turn**.
3. **Deep Research** — `deep_research.rs`. `citation_events` (`:1956-1983`) mints `dr-{n}` where `n = source.number`, matching the `[n]` the synthesis prompt told the model to use (`:1882-1935`, *"you may cite [1], [2], … exactly as listed"*); unread sources get `dr-unread-{status}-{k}` ids, deliberately excluded from the numbered range. Same independent-numbering issue vs. KB grounding as #2.

**Confirmed reachable collision**: `retrieval::wants_grounding` and `web_search`/deep-research availability are decided independently (`sse.rs:735,1175-1333` vs `:1560-1625,1858`) — nothing stops both firing in one turn, in which case the model sees **two unrelated `[1]`s** and the client's single flat, arrival-ordered `turn.citations` array can only honor one of them correctly.

**What "verification" already does — and doesn't**: `verification.rs` is a **whole-answer**, not per-claim, post-hoc pass: it only runs on short (`≤300` output tokens, `MAX_VERIFIABLE_OUTPUT_TOKENS:38`), low-confidence (`<0.75`), previously-unevidenced answers (`decide`, `:111-133`), asks a judge model **one** SUPPORTS/CONTRADICTS/UNRELATED verdict for the **entire answer** against ≤5 snippets (`entailment_prompt`, `:243-273`), and emits one `ChatEvent::Verification{verdict, kb_citations, web_citations, web_allowed}` (`sse_events.rs:119-129`). The module's own doc comment is explicit that real per-claim verification is *"claim extraction plus per-claim retrieval — a different and far more expensive product"* (`verification.rs:32-37`) — **this does not exist and this item does not need to build it**.

`source_validation.rs` is a different, unrelated mechanism: a bounded, **per-segment** (paragraph/table-row, see `paragraphs`/`segments`, `:90-140`) verdict of *generated document text* against **conversation attachments only** (`SourceContext::from_messages`, gated on `CONVERSATION_SCOPE_NOTICE`, `:11,50-81`), used for document-mode correctness/repair (export feature, revision-turn reuse). Its segment-splitting is the closest existing precedent for "claim boundaries" in this codebase, but it is not wired to and doesn't naturally extend to web/KB chat citations — reuse the *idea* (split into paragraph-ish segments), not the module.

---

## 2. Exact change list

Scope note: this does **not** build new retrieval or per-claim extraction. It (a) fixes a real, already-reachable numbering-correctness bug, (b) attaches `sourceGroupId`/`claimId`/`start`/`end` to citations the system already retrieves, derived from data already in hand, and (c) adds domain/provider grouping to the existing chip UI. Everything server-side is a pure post-processing/bookkeeping pass over already-collected data — no new external calls.

### 2.1 Backend — `sse_events.rs`

- Add `pub source_group_id: String` to `ChatEvent::Citation` (non-optional — every producer can always compute one, see 2.2). Update `payload()` (`:270-277`) to add `"source_group_id"`.
- Add `pub claim_id: Option<String>` and `pub start: Option<u32>`, `pub end: Option<u32>` to `ChatEvent::Citation` too, defaulted to `None` at first emission (they're filled in by the anchor pass, 2.4) and serialized only `if Some` (`skip_serializing_if`) to keep the payload small on the common "no anchor yet" frame.
- Add `pub source_group_id: String` to `RecordedCitation` (`:504-510`), copied straight from the event at the recording intercept (`:675-683`).
- Update the `#[derive]`s' existing unit tests around `:1071-1177` (`cite.name()`, `family()`) to include the new fields in the literal construction.

### 2.2 Backend — mint `source_group_id` at each producer (self-contained, low risk)

Add a tiny shared helper (new file `apps/Model Plane/rust/services/model-gateway/src/url_domain.rs`, or a `pub(crate) fn web_domain(url: &str) -> String` in an existing small util module) that ports the frontend's exact behavior (`chat-media-markdown.tsx:530-536`): parse with a WHATWG-URL-equivalent (no `url` crate dependency exists yet in this crate's `Cargo.toml` — either add `url = "2"` (tiny, widely vetted) or hand-roll `scheme://` strip + take-up-to-next-`/` + strip leading `www.`, matching the JS fallback of "return the raw string on parse failure"). Keeping server and client hostname-normalization byte-identical matters because the Sources tab's future domain grouping (2.6) must produce the same group for the same URL on both surfaces.

- `retrieval.rs`: thread `provider` into `add_context_entry` (it already has `source: Option<&Source>` via `sources_by_doc`, the same lookup `build_grounding_facts` uses at `:519-522`) and set `GroundingCitation.source_group_id = format!("kb:{}", provider_slug)` where `provider_slug` is the **un-normalized** input to `source_label` (e.g. `"sharepoint"`, `"gdrive"`) so the id is a stable machine token, not the localized label. Add `pub source_group_id: String` to `GroundingCitation` (`:86-91`) and set it at both push sites (`:376-385` pack-facts loop, `:407-416` candidates loop) via `add_context_entry`.
- **Fix the entries-vs-citations off-by-gap in the same function** (`:447-461`): change `let number = entries.len() + 1` to only assign a bracketed number in the citable branch, using `citations.len() + 1` (i.e. the position the entry is *about to take* in the `citations` vec), and **drop the bracket entirely** from the non-citable branch's rendered line (it already tells the model *"do not cite it"* — giving it an unused number invites exactly the confusion this bug causes). This makes the KB-only case's `[n]` ↔ `citations[n-1]` mapping always correct.
- `tool_loop.rs::web_search_citations` (`:5409-5426`): set `source_group_id: web_domain(&hit.url)`.
- `deep_research.rs::citation_events` (`:1956-1983`): set `source_group_id: web_domain(&source.url)` for both the read and unread branches.

### 2.3 Backend — cross-producer numbering collision (higher risk, see §5/§6)

KB grounding and `web_search`/deep-research can co-occur in one turn (confirmed, §1.2) and today each restarts `[n]` at 1. Recommended fix: a per-turn shared counter.

- Add `fn next_citation_number(&self) -> u32` to `RichEventSink` (`sse_events.rs:443-481`), an `AtomicU32` fetch-add alongside the existing `citations: Arc<Mutex<Vec<RecordedCitation>>>` field.
- `retrieval::ground`/`build_context_entries` needs a handle to this counter to number citable entries (today it runs with no sink reference at all — this is a real signature change: thread `&RichEventSink` or a lighter `&AtomicU32` counter into the grounding call in `sse.rs:1333-1360` and down into `retrieval.rs`'s entry point).
- `tool_loop::gated_web_search_output`'s `KEPT` numbering (`:5511-5525`) starts from the counter's current value instead of always `rank+1`; same for `deep_research`'s `source.number` assignment.
- This is the one piece of the item that touches three modules' call signatures. Recommend landing 2.1/2.2/2.4/2.5/2.6 first (fixes the overwhelmingly common single-source-family turn correctly, plus all of claimId/sourceGroupId/grouping/fallback), and treating 2.3 as a follow-up slice gated on confirming real-traffic frequency of mixed KB+web turns (§5).

### 2.4 Backend — the anchor pass (claimId + span offsets)

New function, e.g. `pub fn anchor_citations(answer: &str, turn_id: &str, recorded: &[RecordedCitation]) -> Vec<ChatEvent>` in a new `citation_anchors.rs` (mirrors `source_validation.rs`'s self-contained-module shape).

- Reuses the **exact** marker grammar the frontend already matches (port `parseInline`'s citation alternative, `chat-media-markdown.tsx:1152`, i.e. `\[(?:\d+(?:\s*,\s*\d+)*)\]`) as a `regex::Regex` (already a workspace dependency, `Cargo.toml:27`, pattern already used this way in `source_validation.rs:317-334`).
- Splits `answer` into paragraph-ish segments using the same idea as `source_validation.rs::paragraphs` (`:90-110`) — blank-line/fence-aware blocks — and assigns each segment a stable `claim_id = format!("{turn_id}:claim-{segment_index}")`.
- For each marker match, for each index `n` inside it: resolve the citation via `recorded.iter().find(|c| c.number == Some(n))` (requires `RecordedCitation`/`ChatEvent::Citation` to also carry the assigned `number: Option<u32>` used to write the prompt's `[n]` — thread this through from 2.2/2.3 the same way `source_group_id` is threaded) — **not** `recorded.get(n-1)`, precisely to survive any remaining producer-ordering quirks. Compute `start`/`end` as the marker's own byte/char offsets within `answer`.
- Emits one **updated** `ChatEvent::Citation` per (citation, first occurrence) with `claim_id`/`start`/`end` now `Some`, reusing the *same* `id` as the original frame.
- Call site: `sse.rs`, right where `assistant_output` (the complete final answer text, confirmed at `sse.rs:2896-2924` next to the `verification::decide` call — same integration point that already needs the whole answer text post-stream) is available, and after `sink.recorded_citations()` (`sse_events.rs:753-770`, already exists, already dedups by url/id) has everything. Emit these frames right before `Usage`/`TurnTiming` go out, same place/pattern as those two (`sse.rs` ~2896-onward through the `emit_and_buffer` calls near `Usage`).
- **Scope decision (flag to owner, §6): first-occurrence only.** `Citation` is a scalar-per-source record; if the model cites the same source at two different, disjoint claims, only the first occurrence's `claim_id`/`start`/`end` are recorded (mirrors `upsertCitation`'s existing first-write-wins dedup, `chat-normalizers.ts:1613-1616` — no frontend change needed for this). The second occurrence still renders a correct chip (the client resolves position via its own regex match, unaffected by whether `claimId` is set) — it just won't carry its own distinct `claimId`. A true one-citation-per-occurrence model would require restructuring `Citation` into a `CitationOccurrence` list, which is a materially bigger change (touches `upsertCitation`, `collectEvidenceSources`, `EvidenceSource`, every consumer that assumes one row per source) — out of scope unless the owner wants it now.

### 2.5 Frontend — carry the new fields through (mostly already done)

- `Citation` type (`chat-types.ts:89-100`): no change needed, fields already exist.
- `upsertCitation` (`chat-normalizers.ts:1613-1616`): **must change** from pure dedup-and-drop to dedup-and-merge, so a second sighting of the same `id` (the anchor-pass re-emission, 2.4) updates `claimId`/`start`/`end`/`sourceGroupId` onto the already-recorded citation instead of being silently dropped:
  ```ts
  export function upsertCitation(citations: Citation[], citation: Citation): Citation[] {
    const index = citations.findIndex((item) => item.id === citation.id || item.url === citation.url)
    if (index < 0) return [...citations, citation]
    return citations.map((item, i) => i === index ? { ...item, ...citation } : item)
  }
  ```
  This is a behavior change from today's true no-op-on-duplicate — check `chat-normalizers.test.ts`/any test asserting the old drop-on-duplicate semantics still hold for genuinely-conflicting duplicate ids (there should be none, since ids are meant to be stable per source).
- `emitCitation`/`normalizeCitation`: no change, already read these keys (§1.1).

### 2.6 Frontend — domain/provider grouping in the chip and the Kilder tab

- `InlineCitationMarker` (`chat-media-markdown.tsx:1055-1086`): group `props.citations` by `sourceGroupId` (fallback to `hostname(citation.url)` when `sourceGroupId` is absent, so old/ungrouped citations degrade gracefully) before rendering the popover list, with one sub-heading per group (domain or provider label) instead of one flat list. Label a KB group (`sourceGroupId` starting `kb:`) with the provider's human name (reuse the same normalization as `source_label`, ported or requested from the payload — simplest: have the KB producer also set `Citation.title`'s existing convention, or send the human label as part of `source_group_id`'s display — recommend a companion `Citation.sourceGroupLabel?: string` field, wire-name `source_group_label`, set once by `retrieval.rs` to the already-computed `source_label()` output, so the frontend never has to re-derive Norwegian/English provider names from a slug).
- `partitionEvidenceSources`/`collectEvidenceSources` (`chat-media-markdown.tsx:113-176`): add a `groupBySourceGroupId` (or extend `partitionEvidenceSources`'s return shape) used by `SourcesPanel` (`ChatPanels.tsx:353-355`) to render domain/provider sub-headings in the Kilder tab, consistent with the inline chip's grouping — same `sourceGroupId ?? hostname(url)` fallback rule so KB `GroundingSource` rows (which don't have `sourceGroupId` from `ChatEvent::Citation` — they're read from the separate `Grounding` event, `ChatKnowledgeGrounding.sources`, `chat-types.ts:161-171`) can also group by `provider` (already present on `ChatGroundingSource`, `:125-135`) without a wire change on that side.

### 2.7 Fallback when a claim has no source

Recommended (safe, in-character with this codebase's anti-overclaim posture — see `chat-types.ts:94-96`'s own comment, `InlineCitationMarker`'s "never guesses" comment, and `ConfidenceNotice`'s deliberate F-04 care about not claiming a check that didn't happen):

- **No invented per-sentence marker.** Do not synthesize a chip, an underline, or a "[?]" glyph on unmarked prose — that would be the client inferring an evidentiary judgment the backend never made, which is the exact thing `Citation`'s own doc comment forbids.
- The only new signal: when a turn had at least one available citation this turn (`turn.citations.length > 0`) **and** the answer's segment count (from the same paragraph split used server-side, 2.4) exceeds the number of segments that contain a marker, surface a single, existing-pattern, quiet note — reuse `ConfidenceNotice`'s established phrasing convention (`ChatMessages.tsx` ~2345-2410) rather than a new UI element: e.g. only alongside the existing confidence line, never a new inline affordance. **This exact behavior is the owner decision in §6** — the safe default absent a decision is to ship 2.1-2.6 with *no* new "uncited claim" UI at all (today's behavior: an unmarked sentence simply renders as plain text, which is already honest) and treat any "flag the gap" affordance as a follow-up once the owner picks a stance.

---

## 3. Tests to add

**Rust (model-gateway), extend existing `#[cfg(test)]` modules in place, matching this crate's convention of colocated tests:**

- `retrieval.rs`: a test asserting a non-citable entry does **not** consume a `[n]` slot and a citable entry after it gets `[1]` (regression test for the off-by-gap fix, 2.2) — extend the existing context-block tests around `:1199-1409`. A test asserting `GroundingCitation.source_group_id == "kb:{provider}"` for a known `Source.source` slug.
- `tool_loop.rs`: extend `web_search_outputs_become_citation_events` (`:10502`) to assert `source_group_id == web_domain(url)` for a couple of distinct hosts, including one where two hits share a domain (asserting they get the **same** group id).
- `deep_research.rs`: extend the existing `citation_events`/numbering tests (near `:3638-3736`) the same way for `dr-{n}` and `dr-unread-*` ids.
- New `citation_anchors.rs` test module: single-marker single-claim case; two markers `[1, 2]` in one sentence (assert both citations get the **same** `claim_id`); a marker in a later paragraph (assert a different, later `claim_id`); a marker index with no matching `RecordedCitation.number` (assert no anchor event is emitted — never a panic/None-unwrap); the same citation `id` cited twice in two different paragraphs (assert only the **first** occurrence's offsets are recorded, per the first-occurrence-only decision in 2.4).
- `sse_events.rs`: extend the `ChatEvent::Citation` unit tests (`:1071-1177`) for the new fields' `payload()` serialization (including the `skip_serializing_if` omission of `claim_id`/`start`/`end` when absent).
- If 2.3 (shared counter) is implemented: an integration-style test in `sse.rs`'s test module constructing a turn with both `wants_grounding` and `web_search` results and asserting the emitted citation `number`s form one continuous, non-overlapping sequence.

**Frontend:**

- `chat-markdown.test.ts` (extend the existing `describe` around `:167-183`): a citation with `sourceGroupId` set groups correctly in `InlineCitationMarker`'s popover (assert sub-heading/grouping DOM, not just flat list); a citation with no `sourceGroupId` falls back to `hostname()` grouping (backward compatibility with citations from a not-yet-updated gateway).
- `chat-normalizers.test.ts`: extend `normalizeCitation` tests (`:59-81`) for `claimId`/`sourceGroupId`/`start`/`end` passthrough (already partially covered — extend, don't duplicate); new tests for `upsertCitation`'s changed merge-on-duplicate-id semantics (2.5): first citation has no `claimId`, second call with the same `id` and a `claimId` set results in one row with the `claimId` merged in, not two rows and not the update dropped.
- `chat-client.test.ts`: extend the citation-mapping test (`:594-615`) with a payload that **does** include `claim_id`/`source_group_id`/`start`/`end` and assert they reach `onCitation` (the current test explicitly asserts the fields are absent — that assertion should be split into a "legacy payload" case and a new "full payload" case, not simply widened, so both wire shapes stay covered).
- `ChatPanels.test.tsx`: `SourcesPanel` renders domain/provider group headings when sources carry `sourceGroupId`/`provider`.
- A resume/replay test (pattern in `use-chat-controller.resume-replay.test.ts`) asserting a citation's anchor-pass update (second `citation` frame, same `id`, now carrying `claimId`) survives a reconnect the same way the original citation event does — this is the scenario the module comment at `use-chat-controller.ts:2911-2925` was written to prevent regressing.

---

## 4. Rollout

- All changes are inside `model-gateway` (Rust) and the frontend SPA — no schema/migration, no new external dependency required for the regex/segment logic (`regex` already in `Cargo.toml:27`); only the optional `url = "2"` crate (or a hand-rolled parser, avoiding the new dependency) for hostname extraction is a new choice to make.
- **Per repo convention (memory: "Build images via the launcher"), rebuild via the launcher's `--from model` path, not a hand-built `docker compose build`** — this crosses the Control Plane env chain the launcher assembles. The Wave-1 rollout note in the plan (`CHAT_PAGE_GAP_PLAN_2026-09-28.md:146`) used exactly this path for the same service; follow the same sequence: launcher model-plane rebuild, then restart the frontend container (dev Vite runs over a bind mount with watching disabled, so a frontend source edit needs a container restart to reach the app, per the same plan note).
- No new feature flag needed: citations are already default-on (`DEFAULT_FEATURES` includes `'citations'`, `chat-client.ts:793`; the `"citations"` SSE family is always requested). The new fields are purely additive on the wire (`skip_serializing_if` for `claim_id`/`start`/`end`), so an old, un-rebuilt frontend or a mid-rollout mixed-version client keeps working unchanged (extra JSON keys are ignored by a client that predates them; a client that expects them and doesn't get them — old gateway — already degrades gracefully, since `InlineCitationMarker`'s grouping falls back to `hostname()`).
- Sequencing recommendation: land 2.1/2.2/2.4/2.5/2.6 (backend fields + anchor pass + frontend grouping) as one deployable slice; land 2.3 (shared cross-producer counter) as a second slice once §5's mixed-turn frequency is checked, since it's the one part touching three modules' function signatures and is more likely to need a second review pass.
- Rebuild list: `model-gateway` (Rust, Model Plane) — new fields, new module, changed function signatures in `retrieval.rs`/`tool_loop.rs`/`deep_research.rs`; `verevonv3` frontend (SPA) — `chat-media-markdown.tsx`, `chat-normalizers.ts`, `chat-types.ts` (comment-only, no field change needed), `ChatPanels.tsx`, `global.css`. The gateway BFF (`apps/gateway`, Rust) is a pass-through for unknown SSE fields already (`sse_events.rs:1-8` — *"the BFF re-streams unknown events verbatim"*) — no BFF change expected, but worth a quick grep of `apps/gateway/src/domains/chat/*.rs` for any place that explicitly allowlists/strips citation payload keys before this ships (not found in this read-only pass; flagged as a pre-merge check, not a known blocker).

---

## 5. Risks and edge cases

- **Mixed-source-turn numbering collision (highest-priority risk, confirmed reachable, §1.2/§2.3)**: KB grounding and web_search/deep-research citations can occupy the same turn and independently restart `[n]` at 1 today. Until 2.3 lands, a turn that mixes both can still show a chip resolving to the wrong source for one of the two families (pre-existing bug, not introduced by this change, but this item is the natural place to fix it since it needs the exact same "stable id, not position" plumbing). Recommend a quick log-based check of how often `wants_grounding` and `web_search_available` are both true in the same turn in production before committing to 2.3's cost.
- **The entries-vs-citations off-by-gap (2.2's KB fix)** is a genuine, narrow, high-confidence bug fix, but touches a function (`add_context_entry`) called from two sites (pack-facts and candidates loops) — verify both call sites' existing tests (`retrieval.rs:1199-1409`) still pass with the renumbering; a couple of those tests assert literal `"[1]"`/`"[2]"` substrings in `context_block` and will need updating if a leading entry in their fixtures is non-citable.
- **First-occurrence-only `claimId` (2.4)**: if a model cites the same source at two disjoint claims, only the first gets a `claimId`. Low practical impact (the chip still renders correctly at both locations via the client's own regex match) but is a real, named simplification — see owner decision in §6.
- **Span offsets are informational, not load-bearing for rendering**: `start`/`end` are computed by the anchor pass against the complete `assistant_output` string, but `parseInline` operates over already-split markdown **blocks** (paragraph/list-item/table-cell substrings), not the whole-answer string — a block's own start offset within the full text isn't tracked today. Do **not** have the client use server `start`/`end` to slice/highlight text directly; keep chip placement exactly as it is now (client's own regex match), and treat `start`/`end` purely as an attached data point (future "jump to citation," analytics, audit). Document this explicitly in the `Citation` type comment so a future implementer doesn't wire them into rendering and hit silent misalignment.
- **`upsertCitation`'s semantics change (2.5)** from strict no-op-on-duplicate to merge-on-duplicate is a behavior change other call sites rely on (`addAssistantCitation`, `appendSearchEvidence`) — re-verify no code path depends on a duplicate citation call being a true no-op beyond field values (e.g. any place that counts `upsertCitation` calls as "citations found" rather than reading array length after).
- **Resume/replay**: the anchor pass's second `Citation` frame for the same `id` must be buffered/replayed exactly like the original (§3's resume test) — the existing `SinkResume` buffering (`sse_events.rs:483-492`) is generic over any `ChatEvent`, so this should work unmodified, but it's untested for a *repeated* event with the same logical id today and deserves explicit coverage.
- **KB `sourceGroupLabel` i18n**: `source_label()` (`retrieval.rs:310-339`) returns English-only labels ("Microsoft 365", "Google Workspace") — check whether the Norwegian-first UI requirement (owner direction, plan header) wants these translated before display, or whether provider names are treated as proper nouns (like existing `hostname()` domain labels, which are never translated either) — likely fine as-is, but worth a one-line confirmation rather than an assumption.

---

## 6. Genuine owner decisions

1. **First-occurrence-only `claimId`/span (2.4) vs. a full one-citation-per-occurrence model.** The simple version (ship now) means a source cited twice at different claims only carries claim/span metadata for the first mention. The full version means restructuring `Citation` into a list of occurrences, touching `upsertCitation`, `collectEvidenceSources`, `EvidenceSource`, and every consumer that assumes one row per source — materially larger. **Only matters if the product wants per-occurrence audit/analytics; the chip itself renders correctly either way.**
2. **"Claim has no source" fallback (2.7)**: ship with no new UI (today's behavior — unmarked prose stays plain text), or add a quiet, existing-pattern note (reusing `ConfidenceNotice`'s phrasing convention) when a turn had sources available but some segments went uncited. This changes what the reader is told about answers that mix cited and uncited claims — a genuine product-honesty call, not an engineering one.
3. **Cross-producer numbering fix (2.3) priority**: fix now (bigger, three-module signature change) vs. defer until real-traffic data confirms how often KB grounding and web_search/deep-research actually co-occur in one turn. Affects whether this item ships as one slice or two.
4. **KB provider label language** (§5 last bullet): translate provider names for the Norwegian-first UI, or leave them as English proper nouns like domain names already are.

---

## 7. Size and Wave-2 dependencies

- **Size: M.** Backend: new module (~150-250 lines) + surgical edits to three existing files (`sse_events.rs`, `retrieval.rs`, `tool_loop.rs`, `deep_research.rs`) + one new small util. Frontend: edits to 3-4 existing files, no new files, no new dependency. 2.3 (shared counter) alone would push a piece of this toward M/L if taken as a hard requirement rather than a follow-up — recommend treating it as such to keep the core item at M.
- **Dependencies on other Wave-2 items**: none block this one. It's independent of "schedule this," the source-scope filter, the attach-from-connector picker, persona instructions, the editable plan gate, the subagent ceiling, and "forward an email to create a task" (`CHAT_PAGE_GAP_PLAN_2026-09-28.md:154-161`).
- **What could reuse this item's work**: the source-scope filter (Perplexity Focus-style org/web/both, `:83,156`) will want exactly the `kind`/`sourceGroupId` distinction this item introduces on `Citation`/`GroundingCitation`, so landing this first makes that filter cheaper. No other Wave-2 item is blocked waiting on this one.