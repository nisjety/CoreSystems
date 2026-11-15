# T3 / 01 Chat experience: cross-source comparison

207 reference records from 12 sources were grouped into 43 capability clusters (none excluded), then mapped to 77 code-verified Verevon baseline records plus targeted greps done in this pass. Verevon status by cluster: 7 shipped, 23 partial, 12 absent, 1 unknown. Tiers: 8 priority, 17 recommended, 9 nice-to-have, 2 skip, 7 n/a (strengths). Machine-readable detail: `01-chat-experience.clusters.json`.

## Shared baseline (table-stakes)
Streaming with typed deltas (C-001), rich Markdown/math/diagram rendering (C-004), edit/regenerate with persisted versions (C-008), branch/fork with lineage (C-009, 8 of 12 sources), a rich composer with slash menu and typed tokens (C-022), conversation organisation (C-027, 9 of 12), cross-thread search and agent recall (C-028, 7 of 12), projects with instructions, knowledge and members (C-029, 7 of 12), and share links/snapshots (C-030, 8 of 12).

## Best of breed
| Capability | Best source | Why (mechanism in one line) |
|---|---|---|
| Incremental state sync | Antigravity (SRC) | Deltas per collection with index, total length and page bounds; older steps load on scroll |
| Render performance | T3 Code (SRC) + DeepSeek (DOC) | Virtualised list with a 40 px live-follow re-arm; only the trailing blocks re-parse |
| Settled-turn folding | T3 Code (SRC) | A 'Worked for 2m' row hides tool noise and keeps the first and last messages |
| Queue and steer | Claude (SRC) + Manus (SRC) | Visible queue you can reorder or send now; split send button (interrupt / steer / queue) |
| Fork / rewind | Claude (SRC) | Fork with lineage; rewind conversation and files, and the rewind itself can be undone |
| Side chat | Claude + Codex (SRC) | Temporary read-only fork that cannot call tools |
| Ask-user questions | Claude + Antigravity (SRC) | Typed question step (multi-select, rank, free text, skip) that pauses the run and notifies the user |
| Artifact editing | Manus (SRC) | Server-side versions, restore, who edited (AI or user), editing locked while the agent runs |
| Point-and-edit | Manus + T3 Code (SRC) | Comments, marks and tracked changes reach the agent as tagged text blocks that work with any provider |
| Generative UI | Antigravity (SRC) + Codex (DOC) | Locked-down iframe that gets the host's theme tokens; small state store and a follow-up bridge the user must confirm |
| Composer tokens | T3 Code (SRC) | Lexical chips (one per token) over a plain-text prompt |
| Search and recall | Claude (SRC) | Worker-based transcript index, plus a search tool whose snippets the model treats as untrusted |
| Projects | Manus (SRC) + Claude (SRC/DOC) | 37-RPC project service with versioned instructions and roles; switches to retrieval automatically when knowledge outgrows the context |
| Sharing | Manus (SRC) + claude.ai/Perplexity (DOC) | Snapshot without raw connector data, admin policy, replay page; a viewer's follow-up creates a fork |
| Titles | Hermes (SRC) | Instant rule-based title, then a small-model title; precedence derived < llm < user |
| Claim verification | Perplexity (DOC) | Checks each claim against its cited passage after the answer and rates the support |

## Differentiators worth noting
Rewind with undo (C-010), side chat (C-011), structured ask-user questions (C-013), plan dock and end-of-turn recap (C-014), point-and-edit comments to the agent (C-018), inline generative UI with a prompt-back bridge (C-019), shared replay and fork-on-follow-up (C-030), claim-level source checking (C-036), task references the agent browses lazily (C-039).

## Verevon's biggest gaps (priority tier)
1. **Projects (C-029, sig 5):** chat projects live only in browser localStorage. Make Spaces the chat project. Membership lives in the Application Plane and knowledge in Data Plane v2. Spaces instructions must also reach Do runs.
2. **Server-side message tree (C-008, C-009):** regenerate versions disappear on reload, and branches exist only in the browser. Build them in session-core (`parent_message_id` is already reserved in gateway.proto).
3. **Search and recall (C-028):** search is a substring filter over at most 80 loaded threads. Add Postgres full-text search in session-core and a read-only `recall_conversations` tool.
4. **Artifact editing and server-side versions (C-017):** artifacts are read-only to the user, and their history is kept on the client.
5. **Ask-user tool (C-013):** none exists, although the durable pause machinery is already in place.
6. **Render performance (C-005):** no virtualisation, and every token triggers a full re-parse.
7. **Resume/cancel/queue durability (C-002):** these registries live in memory on one replica. The fix is S effort: Redis/Dragonfly plus NATS.

Recommended next: share links (owner-gated, ZDR-refusing), side chat, editable message queue, generative UI inline, typed composer tokens, drafts per thread, attachment limits (1 MB/file today), library via Data Plane documents, organisation (archive/unread/paging), export/import, claim-level citations, thread references, rewind.

Skip: cached fast answers (risk of data crossing tenants; conflicts with ZDR) and messaging channels (Channel Plane is future).

## Verevon strengths (keep)
- **Event contract:** typed ChatEvent taxonomy with a CI coverage gate, and a byte-for-byte BFF relay with no allowlist (C-001).
- **Rendering:** its own Markdown parser, Mermaid in strict mode, and native accessible SVG charts (C-004).
- **Steering and edits:** the model decides whether mid-run input is a redirect or a follow-up. Turns that had effects cannot be edited in place, and stopped is a distinct state (C-007, C-008).
- **Context:** compaction with protected pins, plus an itemised context inspector (C-035).
- **Sources:** the panel separates sources that were read from those that were not, and shows a grounding overview (C-036).
- **Plan card:** an editable plan with authority levels and a written reason (C-014).
- **Work panel:** Work, Sources and Trace views with memory recall (C-040).
- **Modes:** an explicit Ask/Do switch instead of silent escalation (C-034).
- **ZDR handling:** local drafts and caches are purged under ZDR (C-023).

## Caveats
- **Web sources come from docs only.** Records for web-chatgpt, web-claudeai, web-manus and web-perplexity are DOC-only. Where a cluster rests only on DOC/INF records, the rationale says so and confidence is capped at medium (C-031 is low).
- **Some vendors appear twice.** claude/web-claudeai and manus/web-manus are the same vendor, so prevalence counts can overstate how widespread a capability is (C-012, C-033, C-039).
- **Two classes were set by judgement.** C-001 and C-008 are marked table-stakes although they have fewer than 7 source records. C-025 is marked common at n=3. The rationale fields explain why.
- **Statuses from this pass's greps** (they only show what was not found in the repo): side chat absent, no ask_user tool, no getSelection, no chapters/TOC, `ChatAppearancePreview` is dev-only, and per-thread drafts were not found (medium confidence).
- **Unknown, needs a code check:** C-031, whether Application Plane Space rooms already support multi-user agent threads.
- **Shipped does not mean proven live.** Several features are `built_unit_tested` but not proven live, such as citation chip binding and the plan gate. No provider is ZDR-attested, so features that depend on ZDR cannot be shown working live.
