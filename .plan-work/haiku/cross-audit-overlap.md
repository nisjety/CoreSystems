# Cross-audit overlap index: Space hybrid plan vs sibling audits a and b

Worker: haiku (cross-audit overlap). Date: 2026-10-14. Read-only analysis. No product code, no live-tree writes, no commits.

## 0. Sources, citation rules, method

- Space side: `.plan-work/audit.md` (copy of 01-verevon-space-audit.md). Space items are cited as `S-F-t1-nnn`; line refs as `S:<line>` into `audit.md`. Space ADR 1-10 are at `S:1624-1642`, Space 9.4 dependency notes at `S:1646-1655`, workstreams (WS01-WS20) in `.plan-work/workstreams.md`.
- Sibling a: `02-quarry-and-browser-audit.md` (F-t2-nnn, 77 items). Sibling b: `03-model-plane-chat-and-agents-audit.md` (F-t3-nnn, 98 items). Both are in `C:/Users/ImaFernandesDaCosta/AppData/Roaming/Claude/scratch-workspaces/5e23d905-dd91-4518-8ff4-252efcbe0520/b5ff8b3a-437c-4499-9a7f-469de994f319/scratch-2026-10-12-69a4ee/`.
- Citation prefix: `a:` = 02 file line, `b:` = 03 file line, `S:` = audit.md line. Within a cell, `card` = the item card heading or body line in that file; `AppB` = Appendix B index line; `road` = roadmap 9.1 table line; `ADR` = 9.3 line; `quick win` = 9.2 line.
- Sections read in each sibling: executive summary (a:45-123, b:41-112), priority table (a:74-99, b:68-91), roadmap waves and quick wins (a:1258-1381, b:1522-1675), ADR candidates (a:1383-1394, b:1679-1692), dependency notes (a:1396-1407, b:1694-1705), Appendix B (a:1520-1603, b:1813-1917). Appendix D (a:1836) for the Letta note.
- Card bodies read in full: a F-t2-001, 003, 004, 008, 013, 014, 018, 022, 031, 032, 035, 036, 038, 039, 045, 071, 072, 061, 076, 063; b F-t3-004, 005, 006, 008, 009, 012, 013, 015, 016, 018, 020, 025, 026, 031, 032, 043, 046, 049, 060, 064. Every other row is marked "title/road" and relies on the Appendix B title, the roadmap dependency cell and the ADR text.
- Tier, effort, wave and plane are the verified values from Appendix B and the roadmap (note "was X" where the card says a value moved).
- Verdicts: REUSE = the Space item must consume the sibling design rather than build its own. DEP = the Space item waits for the sibling item. CONFLICT = the two designs disagree or compete; an owner decision is needed before either is built. DUP = both propose the same thing; merge. INFO = touch point only.
- Code state: four states per path, computed against `.plan-work/live-tree-status.txt` (quote-aware: git quotes paths with spaces) and `.plan-work/live-tree-tracked-files.txt`. Table in section 8. Line checks against the live tree are in section 9.
- Scratch note: a `classify.sh` in this worker's scratchpad was replaced by a different script between two calls. I did not run the replacement; all state checks were inline commands that read the two snapshot files.

## 1. Summary

- Overlap rows in section 2: **122** (each sibling item listed once under its primary piece; cross-piece touches are noted in the verdict or in section 6).
- Pieces with the most overlap: 6 (browser grants, live view, takeover), 9 (action registry, connectors, plugins), 2 (approvals and grant scopes), 10 (scheduled work and watches), 11 (artifacts, files, sharing).
- Items the Space plan must not build independently (REUSE): session-core search and tree (b F-t3-012), run-control receipts and NATS subject (b F-t3-016), typed run events (b F-t3-049, a F-t2-014), grants and browser run ownership (a F-t2-001, b F-t3-008), ZDR posture source (a F-t2-004), Control-signed per-fire decisions (b F-t3-018), run-action table (b F-t3-025), upload path (b F-t3-017), Data Plane scan hook (a F-t2-039).
- Top CONFLICTS needing an owner decision before the affected Space workstream starts (full list and owners in section 5): client-supplied `space_ref` on browser runs (WS20); queue and steer ruling (WS09, WS15); grant scope vocabulary (WS10); origin vocabulary (WS01, WS02, WS09); policy registry of record (WS03); canvas and version store (WS19); egress enforcement point (WS06); schedule and watch owner (room automations); audit and SIEM pipeline (WS05); resource taxonomy for shared runs (WS14, sharing).
- Code state of the 73 cited paths: 58 COMMITTED, 11 MODIFIED-UNCOMMITTED, 4 UNCOMMITTED-NEW, 0 MISSING (section 8). Session Core migrations: highest tracked number is 0043; 0044 and 0045 are UNCOMMITTED-NEW, so the next free number is 0046 for both the Space plan and b F-t3-012.

## 2. Shared pieces (overlap tables)

Columns: # = row id for cross-reference; Tier · Effort · Wave from Appendix B; Plane from Appendix B. Space items are shown as `S-F-t1-nnn (WS)`.

### 2.1 Session Core thread, run and message authority; non-owner append (WS01, WS02, WS09, WS12, WS13, WS15; S ADR 1)

Space anchor: `S:355`, `S:387`, `S:589`, `S:606`, `S:623`, `S:856`, `S:1624`. The append owner check is `apps/Model Plane/rust/services/session-core/src/grpc.rs:4504` (MODIFIED-UNCOMMITTED; line verified, section 9).

| # | ID | Title | Tier · Effort · Wave | Plane | Overlap with the Space plan (one line) | Verdict | Cite |
|---|---|---|---|---|---|---|---|
| 1.1 | b F-t3-012 | Conversation history foundation in session-core: message tree, full-text index, recall tool, paging | Priority · L · W2 | Model + Frontend | Same index that S-F-t1-018 (WS13) needs: index in Session Core, ZDR turns excluded, erase honoured; the tree and fork model also serve S-F-t1-017 and S-F-t1-039 (WS12, WS13). | REUSE (index, ZDR exclusion) + CONFLICT (card keeps recall owner-scoped as an injection channel, b:530, against S-F-t1-038 agent search under the asker's authority; versions are client-only after an earlier persisted design was rejected, b:528, against S-F-t1-017 revision entries) | b:517-532 (card 522, 528, 530); AppB 1830; road 1562; ADR 9.4#5 b:1700 |
| 1.2 | b F-t3-016 | Multi-replica-safe run control: shared cancel, queued input, durable send receipts | Recommended (was Priority) · M (was L) · W1 | Model + Frontend | Overlaps S-F-t1-040 (per-thread leased queue, member steering) and S-F-t1-033 (idempotency key, send states); card puts cancel and enqueue on a request_id-keyed NATS subject and reuses session-core idempotency for receipts. | REUSE (NATS subject, session-core receipts) + CONFLICT (card says drop the editable queue and split send button unless the owner overrides the round-boundary ruling, b:1252; S-F-t1-032 asks for queue and steer) | b:1245-1255 (card 1252, 1254); AppB 1886; road 1540 |
| 1.3 | b F-t3-013 | Make Spaces the chat project: instructions, knowledge, members, retrieval indicator | Priority · L · W2 | Application + Model + Data | Same room authority as WS01 and WS17 (S ADR 3, ADR 5); card requires any Space picker to resolve through resolve_space_role and a Control-signed decision, never a client-supplied space_ref (b:546), and says it reverses the design that keeps Space conversations in the Space timeline. | CONFLICT (client-supplied `space_ref` in the browser run request, S:665, against server-resolved authority, b:546, b:1685) + REUSE (resolve_space_role) | b:533-548 (card 536, 546); AppB 1831; road 1563; ADR 9.3#3 b:1685 |
| 1.4 | b F-t3-002 | Durable agent loop: checkpoint each round, pause, resume after approval or crash | Priority · L · W1 | Model | S-F-t1-010 durable resume depends on it; S-F-t1-019, -025, -026, -036, -047 wait behind it (S 9.4#5, S:1650). Card not read; 9.4#1 says the loop cannot continue after an approval today and asks for the checkpoint-and-resume core first. | DEP (Space resume rides this core; do not build a Space-side resume) | b:353 (title); AppB 1820; road 1535; b:1696 |
| 1.5 | b F-t3-049 | Typed run lifecycle events and status cues (SSE taxonomy gate) | Recommended · L (was M) · W2 | Model + Frontend | S-F-t1-003 typed seq-numbered projection and S-F-t1-004 run stream need one typed event vocabulary; card says the AG-UI agent-run vocabulary must be covered too and names an SSE taxonomy gate. | REUSE (one event taxonomy and the CI gate) | b:1005-1016 (card 1008, 1012, 1014); AppB 1866; road 1572 |
| 1.6 | b F-t3-001 | One server-assembled run context for Do, Plan and scheduled runs | Priority · M · W1 | Model | Space instructions and thread history must enter the same composed context (S-F-t1-011, -012, -038); 9.4#3 chains it to F-t3-013 and F-t3-060. Title and roadmap only. | REUSE | b:335 (title); AppB 1819; road 1534; b:1698 |
| 1.7 | b F-t3-069 | Rewind with undo preview, side chat and spin-off tasks, built on the message tree | Nice to have · M (was L) · W5 | Model + Frontend | S-F-t1-017 edit and delete revisions depend on the same tree; cannot start before F-t3-012. Title and roadmap only. | DEP (on 1.1) | b:1262 (section 6.1 bullet); AppB 1887; road 1630 |
| 1.8 | a F-t2-014 | Canonical agent run trace: typed events, receipts, correlation envelope | Priority · L · W2 | Model + Ingestion + Frontend | Space WS02 projection (S-F-t1-003) and WS09 run stream must not define a second envelope; card extends `model_plane.v1.Event` with producer_seq and origin fields and has the Application Plane project for realtime. | REUSE (extend `model_plane.v1.Event`; gateway only proxies) | a:542-557 (card 547, 555); AppB 1539; road 1288; ADR 9.3#8 a:1392 |
| 1.9 | a F-t2-035 | Durable hosted browser sessions: idle and absolute TTLs, suspend and resume, view vs control scope | Recommended · L · W3 | Ingestion + Frontend | The Space live preview and hand-off (S-F-t1-020) must not start a second run state machine; card says there is no second run state machine beside session-core and view-only is a scope on owner-checked routes, not bearer URLs. | REUSE | a:842-853 (card 849, 851); AppB 1560; road 1313 |
| 1.10 | a F-t2-038 | Agent presence and run control UX: steer and queue follow-ups | Recommended · L · W3 | Frontend + Model + Application | Same feature as S-F-t1-040 (member-safe mid-run steering) and S-F-t1-032 (queue and steer); card applies steer and queue at the execution-core pause boundary with session-core replay, never bypasses a pending approval, and refuses queued input for ZDR tokens. | DUP (one steer design) + REUSE (ZDR refusal, approval precedence) | a:878-889 (card 885, 887); AppB 1563; road 1316 |
| 1.11 | a F-t2-006 | Reap abandoned agent runs (idle and absolute TTL), cap concurrent runs | Priority · M · W1 | Ingestion | Overlaps S-F-t1-040 leased run queue (lease TTL, reaper) and S-F-t1-027 caps. Title and roadmap only. | REUSE (one lease reaper) | a:406 (title); AppB 1531; road 1273 |
| 1.12 | b F-t3-064 | Public, versioned Agent API; origin-tagged runs; origin and parent id on every run | Recommended · L · W4 | Model + Control + Channel | S ADR 1 proposes `turn_origin` (direct, ambient, automation; S:1624) and S-F-t1-010 adds turn_origin to decisions (S:491); card keeps origin in run metadata and does not widen the six-value thread-origin CHECK (b:1192). | CONFLICT (origin vocabulary, section 5 C4) | b:1185-1196 (card 1188, 1192); AppB 1881; road 1621 |

### 2.2 Approvals, durable resume, grant scopes, quarantine (WS10, WS18, WS20; S ADR 5)

Space anchor: `S-F-t1-010` (WS10, S:485-501; headline `S:66` says there are no once, session or always scopes yet), `S-F-t1-044` (grants once, session, standing, S:986-998), `S-F-t1-006` (WS03, S:419-434).

| # | ID | Title | Tier · Effort · Wave | Plane | Overlap with the Space plan (one line) | Verdict | Cite |
|---|---|---|---|---|---|---|---|
| 2.1 | b F-t3-006 | Approvals v2: scoped decisions, typed cause codes, decision provenance, denial memory, step-up | Priority · L · W2 | Model + Frontend | Same subject as S-F-t1-010 (grant scopes, durable resume, quarantine). Card proposes once, this thread, and always as a reviewed rule (b:426) and says a Space or collaborator grant never widens an owner ACL (b:434). | CONFLICT (three scope vocabularies: this card; S-F-t1-044 once, session, standing; a F-t2-032 turn, conversation, always) + REUSE (Approval.context, ApprovalKind, never-auto seed) | b:421-436 (card 426, 432, 434); AppB 1824; road 1557 |
| 2.2 | b F-t3-003 | Declared tool side-effect classes, never-auto set, per-tool allow, ask or never | Priority · L · W1 | Model | S-F-t1-010 "tool permission policy with ceilings" and S-F-t1-037 per-binding allowlist; 9.4#2 says declared risk comes before approvals and MCP. Title and roadmap only. | REUSE (classes first) + DEP | b:369 (title); AppB 1821; road 1536; b:1697 |
| 2.3 | b F-t3-011 | Finish human-in-the-loop: plan approve-then-continue, ask_user, docked trays | Priority · L · W2 | Model + Frontend | Same card and tray surfaces as S-F-t1-026 (typed input requests and secret drop) and S-F-t1-010 approval card; queue tray overlaps S-F-t1-032. Title and roadmap only. | REUSE (one card and tray vocabulary) | b:501 (title); AppB 1829; road 1561 |
| 2.4 | b F-t3-032 | Org agent policy registry: named modes, setting locks, lockdown, frozen tool snapshots | Recommended · XL (was L) · W2 | Control + Model + Frontend | Second policy registry beside S-F-t1-006 (WS03: effect-policy admin in user-core, locked switches, agent and model allowlist, autonomy posture); card requires a Control-signed, tighten-only registry with locks. WS03 is W1 here, this is W2. | CONFLICT (which registry owns locks and allowlists) | b:801-812 (card 804, 810); AppB 1849; road 1570 |
| 2.5 | b F-t3-074 | Editable permission rules, CEL policy, MCP tool gate | Nice to have · L · W5 | Model | Overlaps S-F-t1-006 and S-F-t1-010 ceilings and presets. Title and roadmap only. | CONFLICT (policy language) | b:1288 (section 6.3 bullet); AppB 1892; road 1635 |
| 2.6 | b F-t3-097 | Tighten-only runtime monitors (asynchronous reviewer, information-flow control) | Nice to have · XL · W5 | Control | S-F-t1-045 member-text screening and room approvals. A model reviewer on the permission path is allowed only tighten-only and shadow-first (ADR 9.3#9). | CONFLICT (standing rule) | b:1296 (section 6.3); AppB 1915; road 1658; ADR b:1691 |
| 2.7 | b F-t3-033 | Content-safety hardening: injection normaliser, one policy check on every memory and skill write | Recommended · L · W2 | Model | S-F-t1-045 member-text screening and DLP; S ADR 3 memory writes. Title and roadmap only. | REUSE | b:813 (title); AppB 1850; road 1571 |
| 2.8 | a F-t2-001 | Production BrowserBroker grants and delegated bearer forwarding | Priority · L · W1 | Model + Frontend | S WS20 (S-F-t1-020) cannot issue room browser grants until this exists; card says a Space must never widen a grant and that the Model Plane owns issuance (a:331). | DEP + REUSE | a:318-333 (card 323, 329, 331); AppB 1526; road 1268; road 9.4#1 a:1398 |
| 2.9 | a F-t2-032 | Per-origin agent consent and action-class grants: child grants, run modes, typed denials | Recommended · XL · W3 | Model + Ingestion + Control + Frontend | Grant scopes overlap S-F-t1-010; card proposes turn, conversation and always scopes where any deny beats any approval (a:809) and parent-bound child grants (a:813). | CONFLICT (scope vocabulary, deny precedence) + REUSE (typed denial with decision source) | a:806-817 (card 809, 813); AppB 1557; road 1310; ADR a:1385 |
| 2.10 | a F-t2-033 | Approval loop UX: keyboard approval card, persisted site rules, attention notifications | Recommended · L · W3 | Model + Frontend + Application | Overlaps S-F-t1-010 approval card and S-F-t1-008 approval notifications. Title and roadmap only. | DUP (approval card) | a:818 (title); AppB 1558; road 1311 |
| 2.11 | a F-t2-036 | Durable tenant-scoped authority and one audit record for every permission and approval decision | Recommended · XL · W3 | Model + Control + Ingestion | Overlaps S-F-t1-010 durable approvals and S-F-t1-031 audit of Space decisions. Card says the approvals table already records decided_by and decision_reason, so the gap is publication, proposing an AuthzDecision event on the verevon.audit.v2 shape (a:861). | REUSE (audit shape and approval capture) | a:854-865 (card 857, 861); AppB 1561; road 1314 |
| 2.12 | a F-t2-024 | Credential handoff: fill_secret and email OTP via integration-corev2 | Priority · L · W3 | Ingestion + Model | S-F-t1-044 secrets broker (in integration-corev2 on SafeClient, S:986-998) and S-F-t1-026 secret drop: same custody question. ADR 9.3#2 leaves integration-corev2 versus a Control secret service open. | CONFLICT (custody owner, decide once) | a:702-716 (title); AppB 1549; road 1306; ADR a:1386 |
| 2.13 | a F-t2-076 | Child browser subagents: read-only browser action class as a strict subset of the parent grant | Nice to have · L · W5 | Model | Overlaps S-F-t1-025 governed delegation (S ADR 5: re-resolve authority per hop). Card reuses run_delegated_child and makes child grants a strict subset of the parent. | REUSE | a:1169 (section 6.5); AppB 1595; road 1363 |

### 2.3 ZDR enforcement and typed refusals (WS06 and every WS that persists or pushes content; S-F-t1-005)

Space anchor: `S-F-t1-005` (WS06, S:1285-1289: commit the untracked refusal catalogue, add a matrix test, guard telemetry labels). Refusal catalogue `apps/Model Plane/rust/crates/mp-contracts/src/zdr_refusal.rs` is UNCOMMITTED-NEW.

| # | ID | Title | Tier · Effort · Wave | Plane | Overlap with the Space plan (one line) | Verdict | Cite |
|---|---|---|---|---|---|---|---|
| 3.1 | a F-t2-003 | Close the gateway search-cache ZDR and tenant hole; per-feature ZDR capability matrix | Priority · M · W1 | Frontend + Ingestion | S-F-t1-005 asks for the refusal catalogue and matrix test; card asks for a matrix generated from route attributes with explicit refusal codes (a:357). Two matrices would drift. | REUSE (one matrix, one refusal-code list) + CONFLICT risk | a:352-369 (card 357-365); AppB 1528; road 1270; ADR a:1388 |
| 3.2 | a F-t2-004 | ZDR-safe telemetry: span sampler, late-ZDR registry, safe and sensitive log fields | Priority · M · W1 | Ingestion + Model + Frontend | S-F-t1-005 content-free telemetry and label guard. Card says do not add a ZDR header or NATS header; posture comes from `Claims::effective_zdr_mode` (a:383). | REUSE (posture source) + CONFLICT risk (any Space ZDR header or NATS carrier) | a:370-387 (card 375, 383); AppB 1529; road 1271 |
| 3.3 | a F-t2-005 | One strict ScrapeOptions contract: inline return is the ZDR path | Priority · M · W1 | Ingestion | Space link previews through Quarry (S-F-t1-052) must use inline return under ZDR; headline says the ZDR scrape path returns nothing today (a:64). | DEP | a:388 (title); a:64; AppB 1530; road 1272 |
| 3.4 | a F-t2-020 | Harden persistent browser profiles: encryption at rest, ZDR enforced inside Quarry | Priority · L · W2 | Ingestion + Control | S-F-t1-028 persistent per-agent computer and S-F-t1-021 confinement would persist state under ZDR. Title and roadmap only. | REUSE | a:638 (title); AppB 1545; road 1294 |
| 3.5 | a F-t2-021 | Secret redaction at capture time: AX and DOM snapshots, screenshots, receipts, logs | Priority · L · W2 | Ingestion + Model | Overlaps S-F-t1-045 secret masking and DLP and S-F-t1-046 files as input. Title and roadmap only. | REUSE | a:654 (title); AppB 1546; road 1295 |
| 3.6 | a F-t2-050 | Fail-closed PII redaction stage for scraped and crawled content | Recommended · L · W3 | Ingestion + Model + Control | Overlaps S-F-t1-045 DLP. Title and roadmap only. | REUSE | a:1022 (title); AppB 1575; road 1326 |
| 3.7 | a F-t2-048 | Org-scoped URL cache with explicit freshness; cache-only no-egress mode | Recommended · M · W3 | Ingestion + Data | Space link previews (S-F-t1-052) and the per-feature ZDR semantics in ADR a:1388. Title and roadmap only. | DEP | a:998 (title); AppB 1573; road 1324 |
| 3.8 | b F-t3-005 | Unblock ZDR: attest one provider, run the live proof | Priority · M (was S) · W1 | Model + Control | Every Space ZDR turn is refused until one ZDR-capable provider is attested (card: with no attested provider every ZDR turn is refused). S-F-t1-005 depends on it. | DEP | b:403-420 (card 410, 416); AppB 1823; road 1538 |
| 3.9 | b F-t3-004 | Deliver the AI-action audit trail, then hash chain, compliance API and SIEM export | Priority · L (was M) · W1 | Model + Control | Overlaps S-F-t1-031 org audit trail (Space decisions and agent invocations) and SIEM export. Card: audit-core is Control, rows are content-free so ZDR is kept, and the session-core publisher fix must deploy first. | REUSE (audit-core owns the API; content-free rows) + DUP (SIEM export designed in three places, see 7.11) | b:385-402 (card 390, 396-398); AppB 1822; road 1537 |
| 3.10 | b F-t3-075 | Privacy by construction and a closed telemetry contract | Nice to have · L · W5 | Model + Control | Overlaps S-F-t1-005 telemetry label guard. Title and roadmap only. | REUSE | b:1290 (section 6.3); AppB 1893; road 1636 |
| 3.11 | b F-t3-062 | Enterprise compliance posture: retention windows, legal hold, sharing policy, telemetry scrub | Recommended · XL (was L) · W3 | Control + Model + Support | S-F-t1-051 retention policies and S ADR 8 (legal hold, retention and ZDR precedence). Title and roadmap only. | REUSE (ADR 8 ordering) | b:1161 (title); AppB 1879; road 1602 |
| 3.12 | b F-t3-041 | Gemini provider family (Vertex, EU region) with ZDR and residency attestation | Recommended · L · W3 | Model | A shared room cannot use a provider for ZDR or residency until it is attested (S ADR 8). Title and roadmap only. | INFO | b:909 (title); AppB 1858; road 1590 |

### 2.4 Agent-work visibility, run streams, AG-UI, steer and cancel (WS09; WS02 realtime)

Space anchor: `S-F-t1-004` (S:403-418), `S-F-t1-003` (WS02, S:387-402), `S-F-t1-035` (S:869-882), `S-F-t1-009` (S:469-484), `S-F-t1-056` (S:1142-1154), `S-F-t1-020` (room preview, S:659-675).

| # | ID | Title | Tier · Effort · Wave | Plane | Overlap with the Space plan (one line) | Verdict | Cite |
|---|---|---|---|---|---|---|---|
| 4.1 | b F-t3-015 | Attention model: closed run-state vocabulary, typed alerts, Web Push, shell stream, PWA | Priority · XL · W2 | Model + Application + Frontend | S-F-t1-004 activity strip, S-F-t1-009 needs-you aggregate, S-F-t1-035 content-free Web Push, S-F-t1-056 needs-input state: one run-state enum is needed. Card says every notification is content-free because ZDR delivery is unproven (b:578). | CONFLICT (state names: this card's enum vs the Space needs-input and working vocabulary) + REUSE (content-free wake; S:879 needs a Control carve-out) | b:565-580 (card 572, 576, 578); AppB 1833; road 1565 |
| 4.2 | b F-t3-035 | Live computer panel with typed viewers (browser, read-only terminal, files) and PTY exec | Recommended · L · W4 | Frontend + Model | S-F-t1-020 room preview card that expands to an interactive view, and S-F-t1-028 per-agent computer; one viewer registry is needed. Title and roadmap only. | REUSE (one viewer registry) | b:837 (title); AppB 1852; road 1612 |
| 4.3 | b F-t3-026 | Human takeover and hand-back from live views: logins, 2FA, CAPTCHA | Recommended · M (was L) · W3 | Model + Ingestion + Frontend | Same take-over and control indicator as S-F-t1-020; card says Quarry's single control-mode state already covers arbitration and that the ChallengeSignal is not consumed today (b:736). | REUSE (Quarry control mode, ChallengeSignal) | b:729-740 (card 736, 738); AppB 1843; road 1582 |
| 4.4 | b F-t3-048 | Readable long runs: density levels, docked plan strip, tool narration | Recommended · M · W3 | Frontend + Model | Overlaps S-F-t1-004 activity, plan and tool cards. Title and roadmap only. | DUP | b:993 (title); AppB 1865; road 1593 |
| 4.5 | b F-t3-034 | Trajectory ledger with exact model-visible request capture and timing waterfall | Recommended · XL · W3 | Model + Frontend | Overlaps the S-F-t1-004 run trace and S-F-t1-027 answer-quality surface. Capturing model-visible requests is a content-persisting boundary, so ZDR threads must be refused (ZDR rule; card not read). | CONFLICT (ZDR capture) | b:825 (title); AppB 1851; road 1585 |
| 4.6 | b F-t3-021 | Transcript performance: block-level streaming render, virtualised timeline, paged reads | Priority · M · W1 | Frontend + Model | S-F-t1-034 scalable room timeline (virtualised rows, scroll anchoring, paging); both need one virtualised list. Title and roadmap only. | DUP (one virtualised list component) | b:647 (title); AppB 1838; road 1543 |
| 4.7 | b F-t3-050 | Scrubbable browser-run replay with per-action briefs | Recommended · M · W4 | Frontend + Ingestion + Model | S-F-t1-039 run permalinks and the S-F-t1-020 room browser. Title and roadmap only. | INFO | b:1017 (title); AppB 1867; road 1617 |
| 4.8 | a F-t2-071 | Render MCP App resources in an opaque-origin sandbox; retire the permissive web-view iframe | Nice to have · L · W5 | Frontend + Model | S-F-t1-014 shared viewer registry. Card says the opaque-origin srcdoc sandbox primitive is shipped and deletes the unused WebPageViewer first. | CONFLICT risk (do not reintroduce a permissive iframe in the Space viewer registry) + REUSE (sandbox primitive) | a:1155 (section 6.2); AppB 1590; road 1358 |
| 4.9 | a F-t2-072 | Share an agent run and its evidence with Space members through the resource-grant authority | Nice to have · L · W5 | Application + Ingestion + Control | Space run visibility (S-F-t1-004) and Space-as-subject grants (S-F-t1-057). Card: enforce the viewer grant at the gateway, never widen an owner-plane ACL through a Space; user-core's taxonomy classes quarry_run as team_shared (a:1159). | CONFLICT (resource taxonomy, section 5 C18) + REUSE (no ACL widening) | a:1159 (section 6.3); AppB 1591; road 1359; ADR a:1392 |

### 2.5 Agent memory, audience labels, Letta (WS17; S ADR 3)

Space anchor: `S-F-t1-012` (WS17, S:519-535: source labels, audience-bound memory, owner-signed declassify), `S-F-t1-011` (S:502-518), S ADR 3 (S:1628: containment slice first; Letta receives the same filter). Letta is mentioned only in a:969 and a:1836; the 03 file has no Letta reference.

| # | ID | Title | Tier · Effort · Wave | Plane | Overlap with the Space plan (one line) | Verdict | Cite |
|---|---|---|---|---|---|---|---|
| 5.1 | b F-t3-060 | Scoped memory for Spaces, Projects and named agents with isolation | Recommended · L · W3 | Model + Frontend | Overlaps S-F-t1-012 and S ADR 3. Card step (1) is a fail-closed exclusion of Space-origin threads from user-scope extraction and recall, which is the ADR 3 containment slice (b:1144). Label schemas differ: scope kind and id (b:1140) against space_ref, audience hash and revision (S:533). | REUSE (containment slice has one owner) + CONFLICT (label schema) | b:1137-1148 (card 1140, 1144); AppB 1877; road 1600; ADR 9.4#3 b:1698 |
| 5.2 | b F-t3-020 | User and org memory controls: on, pause, reset, org policy | Priority · L (was M) · W1 | Model + Control + Frontend | S ADR 3 containment must be enforced at the same points: Dreaming extractor, prefetch, save_memory, recall_memory, skill review (card). | REUSE (enforcement points) | b:629-646 (card 634, 642); AppB 1837; road 1542 |
| 5.3 | b F-t3-061 | Personalisation that tailors itself: standing profile in the cached prefix | Recommended · L (was M) · W3 | Model + Control + Frontend | A profile built from Space turns would carry room facts into personal scope, which S ADR 3 forbids. Title and roadmap only. | CONFLICT risk | b:1149 (title); AppB 1878; road 1601 |
| 5.4 | a F-t2-045 | Memory pollution guard: taint web or browser runs, skip or quarantine memory writes | Recommended · M · W2 | Model | S ADR 3 and S-F-t1-012 review-then-share. Card taints the thread window, adds a save_memory preflight, a pending path for Dreaming, suppresses the Letta mirror (a:969), and lets only the memory owner promote (a:971). | REUSE (taint flag, owner-only promotion) | a:962-973 (card 965, 969, 971); AppB 1570; road 1300; App D a:1836 |
| 5.5 | b F-t3-007 | Govern the skill-learning loop: learned skills start as owner-scoped proposals | Priority · L · W1 | Model + Control + Frontend | Same proposal-then-review gate needed for S-F-t1-046 agent-proposed pinned knowledge and S-F-t1-065 governed Space skills. Title and roadmap only. | REUSE | b:437 (title); AppB 1825; road 1539 |
| 5.6 | b F-t3-094 | Self-maintained team knowledge wiki generated from activity | Nice to have · XL · W5 | Data | S-F-t1-046 agent-proposed pinned knowledge and S-F-t1-012 team knowledge scopes; automatic writes conflict with review-then-share (S ADR 3). Title and roadmap only. | CONFLICT risk | b:1322 (section 6.5); AppB 1912; road 1655 |
| 5.7 | b F-t3-093 | Grow the corpus from research (promote-on-use) | Nice to have · L (was M) · W5 | Data | S-F-t1-046 knowledge proposals. Title and roadmap only. | CONFLICT risk | b:1320 (section 6.5); AppB 1911; road 1654 |

### 2.6 Browser grants, the agent browser path, live view, takeover, challenge handling (WS20, WS06, WS02)

Space anchor: `S-F-t1-020` (WS20, S:659-675: `space_ref` on browser runs, room preview card, hand-off and take-over, challenge hand-off), `S-F-t1-021` (WS06, S:676-690), `S-F-t1-028` (S:791-803). Quarry headline facts: a:61 (takeover enforced in the gateway, not Quarry), a:68 (viewport relaunch kills all sessions).

| # | ID | Title | Tier · Effort · Wave | Plane | Overlap with the Space plan (one line) | Verdict | Cite |
|---|---|---|---|---|---|---|---|
| 6.1 | a F-t2-013 | Co-browsing: takeover enforced inside Quarry (phase 1 is S, in W1) | Priority · L (phase 1 S) · W2 | Ingestion + Model + Frontend | Space take-over and control indicator (S-F-t1-020) assume Quarry enforces control_mode; today the Quarry step handler never reads it (a:61) and the gateway enforces. Phase 1 closes the gap and must land before any Space take-over is proven. | DEP (take-over waits for phase 1) + REUSE (Quarry is the enforcer) | a:524-541 (card 529-537); AppB 1538; road 1287; quick win a:1380 |
| 6.2 | a F-t2-011 | Free human browsing: human-session policy and omnibox | Priority · L · W2 | Ingestion + Model + Frontend | WS20 human and agent sharing one session: grant binding and navigation policy for human browsing are open (ADR a:1387). Title and roadmap only. | CONFLICT risk + DEP | a:492 (title); AppB 1536; road 1285; ADR a:1387 |
| 6.3 | a F-t2-012 | Chat beside the browser: dockable browser panel bound to the chat run | Priority · L · W2 | Frontend | The room browser card and modal (S-F-t1-020, WS20) would be a second browser panel. Title and roadmap only. | DUP (one component) | a:508 (title); AppB 1537; road 1286 |
| 6.4 | a F-t2-027 | Honest bot-block handling: closed block taxonomy, user-visible blocked state, no stealth code | Recommended · M · W1 | Ingestion + Model + Frontend | Space challenge handling (S-F-t1-020 turns Quarry's challenge classification into a hand-off, S:665) and b F-t3-026 CAPTCHA takeover. Title and roadmap only. | REUSE (taxonomy; no challenge bypass) | a:746 (title); ADR a:1389; AppB 1552; road 1277 |
| 6.5 | a F-t2-022 | Org-pinned, tighten-only web policy: tri-state gates as signed claims and grant fields | Priority · L · W2 | Control + Model + Ingestion | Room browser domain fence and S-F-t1-027 admin allow and block lists. Card: requests, Spaces and the Model can only tighten (a:675); the policy of record is still open (ADR a:1385). | REUSE (tighten-only) + CONFLICT (policy of record: org-core, capability-core, or the user-core effect policy of WS03, section 5 C5) | a:670-685 (card 675, 683); AppB 1547; road 1296; ADR a:1385 |
| 6.6 | a F-t2-007 | Stop viewport-triggered browser relaunches: per-page emulation, process pool | Priority · M · W1 | Ingestion | One shared Chromium serves every session and a viewport change clears all pages (a:68). Room browser runs (S-F-t1-020) would share that failure. Title and roadmap only. | DEP | a:424 (title); a:68; AppB 1532; road 1274 |
| 6.7 | a F-t2-059 | Governed remote browser backends behind an egress-containment proof | Recommended · XL · W4 | Ingestion | S-F-t1-020 and S-F-t1-028 need a remote or isolated backend later; the containment proof's location is open (ADR a:1394). Title and roadmap only. | CONFLICT risk (section 5 C8) | a:1106 (title); AppB 1582; road 1341; ADR a:1394 |
| 6.8 | a F-t2-060 | Isolate each browser session or tenant in its own container or VM | Recommended · XL · W4 | Ingestion | S-F-t1-028 per-agent computer, S ADR 6, and b F-t3-009 microVM backend all propose a substrate; ADR a:1394 lists this as the substrate decision. Title and roadmap only. | CONFLICT (substrate decision, section 5 C8) | a:1118 (title); AppB 1583; road 1342; ADR a:1394 |
| 6.9 | b F-t3-008 | Agent browser in chat and Do mode on real broker grants: per-site consent, admin allow and block lists, model-blind logins | Priority · XL (split) · W2 | Model + Ingestion + Control | This is the room browser's engine (S WS20). Card: the gateway mints a grant under the user's delegated BrowserBroker bearer, never from request JSON; admin lists belong to Control; Space decisions may only narrow, block wins (b:466). | REUSE (ownership and narrowing rules) + CONFLICT (client-supplied `space_ref`, S:665, section 5 C1) | b:453-468 (card 458, 466); AppB 1826; road 1558; ADR b:1686 |
| 6.10 | b F-t3-081 | Teach mode: record a hosted-browser workflow and distil it into a reviewed skill | Nice to have · XL (was L) · W5 | Ingestion + Model | Recording in WS20 and S-F-t1-065 governed skills. Title and roadmap only. | INFO | b:1310 (section 6.5); AppB 1899; road 1642 |

### 2.7 ZDR-safe telemetry, cost and budgets (WS06; S-F-t1-027, S-F-t1-031)

Space anchor: `S-F-t1-027` (S:778-790: per-room and per-agent cost; session-core reports run tokens as 0; cost-core ledger has no Space or agent dimension), `S-F-t1-031` (S:817-829: usage ledger export; cost-core entries get space_ref and agent_ref at S:825), `S:786` (carry space_ref through USAGE_ENVELOPE).

| # | ID | Title | Tier · Effort · Wave | Plane | Overlap with the Space plan (one line) | Verdict | Cite |
|---|---|---|---|---|---|---|---|
| 7.1 | a F-t2-008 | Enforce cost and runtime budgets in the Quarry executor; meter browser time; clamp crawls | Priority · L · W2 | Ingestion + Model + Control | Space per-room and per-agent budgets (S-F-t1-027). Card: cap = min(request, grant claim, org default), fail closed; one ledger key per run; ZDR runs accumulate in memory (a:455). | REUSE (min rule, one ledger key) + CONFLICT risk (a second budget ledger) | a:442-457 (card 447, 455); AppB 1533; road 1283; ADR a:1385 |
| 7.2 | a F-t2-029 | Quarry metrics recorder, drain, span vocabulary, alerts | Recommended · L · W2 | Ingestion + Control | Overlaps S-F-t1-031 usage and telemetry exports; the span vocabulary must match the ZDR sampler (7.1 and 3.2). Title and roadmap only. | INFO | a:770 (title); AppB 1554; road 1299 |
| 7.3 | a F-t2-046 | Complete web-usage metering: price table, plan keys, outcome attributes | Recommended · L · W3 | Control + Ingestion + Frontend | S-F-t1-027 cost attribution and S-F-t1-031 usage ledger export: one meter should serve room cost. Title and roadmap only. | REUSE (one meter) | a:974 (title); AppB 1571; road 1322 |
| 7.4 | a F-t2-062 | Budgeted research job contract with run-context cost attribution | Recommended · M · W4 | Model + Ingestion | S-F-t1-027 per-agent cost attribution; cost-core has no Space or agent dimension (S:786, S:825). Title and roadmap only. | REUSE | a:1130 (title); AppB 1584; road 1344 |
| 7.5 | b F-t3-028 | Live run-cost meter with pre-run estimate, cost by token category, quota display | Recommended · L · W2 | Model + Control + Frontend | The room cost line prints nothing because session-core reports run tokens as 0 (S:778-790). Meter depends on F-t3-022 and F-t3-042 (road). | DEP + REUSE (meter design) | b:753 (title); AppB 1845; road 1568 |
| 7.6 | b F-t3-022 | Prompt-cache discipline: measured hit rate, stable tiers, lineage-scoped keys | Priority · M · W1 | Model | Cache levers under S-F-t1-027 and a prerequisite for 7.5. Title and roadmap only. | DEP | b:665 (title); AppB 1839; road 1544 |
| 7.7 | b F-t3-023 | Org and role model entitlements: Control decides, Model enforces at admission | Priority · L (was M) · W1 | Control + Model + Frontend | S-F-t1-006 agent and model allowlist and S-F-t1-027 budgets. ADR b:1690 says Control owns allowed_models and Model fails closed. | REUSE (Control owns; fail closed) + CONFLICT risk (a Space allowlist in user-core) | b:683 (title); AppB 1840; road 1545; ADR b:1690 |
| 7.8 | b F-t3-068 | Policy and data-flow transparency: effective-policy origin view, egress catalogue | Recommended · L (was M) · W3 | Frontend + Control | S-F-t1-006 effective-policy view and S-F-t1-021 egress. Title and roadmap only. | REUSE | b:1233 (title); AppB 1885; road 1603 |
| 7.9 | b F-t3-079 | Typed feature-flag and settings registry served by Control | Nice to have · M · W5 | Control | Space rollout flags (S-F-t1-006) and the feature-flag and rollout section of the Space plan. Title and roadmap only. | REUSE (one flag registry) | b:1292 (section 6.3); AppB 1897; road 1640 |
| 7.10 | b F-t3-083 | Run admission controls: credit-aware pause, org-level agent pause switch | Nice to have · L (was M) · W5 | Control + Model | S-F-t1-006 locked switches and S-F-t1-027 budgets. Title and roadmap only. | REUSE | b:1294 (section 6.3); AppB 1901; road 1644 |
| 7.11 | a F-t2-073 | Control governance extras: SIEM export, approval preferences, privacy-safe usage events | Nice to have · L · W5 | Control | S-F-t1-031 SIEM export and usage ledger export: a third SIEM design beside 3.9 (b F-t3-004). Card: audit-core exposes only a pull API today (a:1160). | DUP (merge into one export) | a:1160 (section 6.3); AppB 1592; road 1360 |

### 2.8 Sandbox confinement and egress (WS06; S-F-t1-021, S-F-t1-028; S ADR 6)

Space anchor: `S-F-t1-021` (S:676-690: real network enforcement, audience-bound egress), `S-F-t1-028` (S:791-803: persistent per-agent computer), S ADR 6 (S:1634: Quarry keeps the network; local executor needs a registered plane and a Control device contract). Space items are shown at the audit level; the Space plan has not yet fixed the enforcement point.

| # | ID | Title | Tier · Effort · Wave | Plane | Overlap with the Space plan (one line) | Verdict | Cite |
|---|---|---|---|---|---|---|---|
| 8.1 | b F-t3-009 | Per-thread sandbox computer behind a SandboxBackend seam; thread workspace on bwrap first | Priority · XL · W2 | Model + Control | S-F-t1-028 persistent per-agent computer and S ADR 6. Card: a thread workspace needs its own Control decision and must not alias a Space workspace (b:482). Ledger 2026-08-22 deferred the SandboxBackend trait (apps/Model Plane/docs/decisions/ledger.md:1358). | CONFLICT (persistence scope: thread versus per-agent; trait reversal) + REUSE (lease, workspace CAS) | b:469-484 (card 474, 480, 482); AppB 1827; road 1559; ADR b:1683 |
| 8.2 | b F-t3-043 | Sandbox egress tiers and credential-injecting egress proxy | Recommended · XL (was L) · W3 | Model + Control | S-F-t1-021 audience-bound egress and S ADR 6 (sandboxes have no direct network; egress goes through Quarry). Card: a closed tier enum in the Control-signed Space capability decision, enforced by netns or firewall reusing Quarry's dns_guard (b:940). | CONFLICT (enforcement point, section 5 C8) | b:933-944 (card 936, 940); AppB 1860; road 1592; S:1634 |
| 8.3 | b F-t3-024 | Computer use inside the agent's sandbox desktop | Recommended · XL · W4 | Model + Endpoint Plane | S-F-t1-028 and S ADR 6 (a registered plane first). The card names "Endpoint Plane" as owner (ADR b:1684). That folder exists in the repo (COMMITTED), but CLAUDE.md lists only the focused planes. | CONFLICT (plane registration, section 5 C20) | b:705 (title); AppB 1841; road 1611; ADR b:1684 |
| 8.4 | b F-t3-085 | Sandbox computer extensions: persistent machine mode, execution-target picker | Nice to have · L · W5 | Model | S-F-t1-028 persistent per-agent computer; depends on F-t3-009. Title and roadmap only. | DUP / DEP (on 8.1) | b:1302 (section 6.5); AppB 1903; road 1646 |
| 8.5 | b F-t3-084 | Per-Space environment specs and prebuilt sandbox images | Nice to have · L · W5 | Model | S-F-t1-028 and S-F-t1-064 room templates; depends on F-t3-009. Title and roadmap only. | DEP (on 8.1) | b:1300 (section 6.5); AppB 1902; road 1645 |
| 8.6 | a F-t2-058 | Scrape request context and egress tiers: country egress, policy-gated escalation | Recommended · XL · W4 | Ingestion + Control | S-F-t1-021 egress tiers. A second egress-tier vocabulary (country and locale tiers) beside 8.2's off, registries, allowlisted, all. Title and roadmap only. | CONFLICT (vocabulary, section 5 C8) | a:1094 (title); AppB 1581; road 1340 |
| 8.7 | a F-t2-063 | Code-mode browser control in a bwrap'd child with network disabled | Nice to have · L · W4 | Model | Matches S ADR 6 sandbox posture. Card: run the engine as a bwrap'd child with network off and one approval per Quarry step (a:1173). | REUSE | a:1173 (section 6.5); AppB 1600; road 1345 |
| 8.8 | a F-t2-077 | Design spike: drive the user's own Chrome through an attested extension (parked) | Nice to have · XL · W5 | Ingestion | S ADR 6: a local executor needs a registered plane and a Control device-identity contract before any code (S:1634). Parked by owner ruling 2026-09-28. | REUSE (park) | a:1170 (section 6.5); AppB 1596; road 1364; ADR a:1394 |

### 2.9 Capability-core tool and action registry, model eligibility, connectors, plugins (WS18; WS13 command layer; WS07 and WS14 viewers)

Space anchor: `S-F-t1-019` (WS18, S:640-658: run-bound grants; S:652 notes runActionTarget accepts only tickets.create), `S-F-t1-037` (S:895-907: tool cockpit), `S-F-t1-041` (WS13, S:947-959: command layer on the typed action registry), `S-F-t1-065` (S:1259-1271: governed skills and plugin bundles), `S-F-t1-014` (WS07, S:553-568: shared viewer registry).

| # | ID | Title | Tier · Effort · Wave | Plane | Overlap with the Space plan (one line) | Verdict | Cite |
|---|---|---|---|---|---|---|---|
| 9.1 | b F-t3-025 | Open the typed action registry to the agent: classify actions, admit reads, per-run scoped tokens | Recommended · M · W2 | Frontend + Model | WS18 (S-F-t1-019) and S-F-t1-041 both need it. Card: runActionTarget accepts only tickets.create (apps/Control Plane/user-core/internal/spaces/run_action_decision.go:340-347) and proposes one per-action table keyed by the catalog schema hash (b:724). Model-executable actions are 8 of 168 (b:720-724). | REUSE (one run-action table) + CONFLICT risk (a Space-only table) | b:717-728 (card 720, 724); AppB 1842; road 1567 |
| 9.2 | b F-t3-056 | Connectors: suggestion cards, resumable connect, editable write confirmation | Recommended · L · W3 | Model | S-F-t1-037 tool cockpit (per-room MCP, person-bound sign-in) and S-F-t1-044 draft-first sends: write-confirmation levels against draft-first. Title and roadmap only. | CONFLICT (outward-action confirmation, section 5 C19) | b:1089 (title); AppB 1873; road 1598 |
| 9.3 | b F-t3-057 | Custom connectors: probe-and-test add flow, secret-reference MCP auth | Recommended · L · W4 | Model + Ingestion | S-F-t1-037 per-room MCP enablement and credential mode. Title and roadmap only. | REUSE | b:1101 (title); AppB 1874; road 1619 |
| 9.4 | b F-t3-010 | Connected apps that work in chat: Gmail, Outlook, Drive, Calendar reads; @-scoping; approved writes | Priority · L · W2 | Model + Ingestion + Control | S-F-t1-037 and S-F-t1-044 per-person connections; S-F-t1-046 files as input. Sibling ADR 9.3#10 asks how personal mailbox content in a shared Space stays away from other members (b:1692). | CONFLICT (viewer-bound connector calls needed, section 5 C19) | b:485 (title); AppB 1828; road 1560; ADR b:1692 |
| 9.5 | b F-t3-059 | Plugin manifest loader with supply-chain pins; org catalogue | Recommended · L · W4 | Model + Control | S-F-t1-065 governed Space skills and plugin bundles (ecosystem manifests). Sibling ADR 9.3#7 proposes a three-tier library and pins (b:1689). | CONFLICT (two manifest and catalogue designs) | b:1125 (title); AppB 1876; road 1620; ADR b:1689 |
| 9.6 | b F-t3-058 | One versioned lifecycle for authored behaviours: skills, slash templates, presets | Recommended · XL (was L) · W3 | Model + Frontend | S-F-t1-065 and S-F-t1-041 slash commands. Title and roadmap only. | CONFLICT risk | b:1113 (title); AppB 1875; road 1599 |
| 9.7 | b F-t3-077 | One action-registry-driven command surface: palette, slash commands, shortcut sheet, double-Escape stop | Nice to have · M · W5 | Frontend | Same design as S-F-t1-041 (Cmd or Ctrl+K palette, slash commands, shortcut sheet). Merge into one slice. | DUP | b:1326 (section 6.6); AppB 1895; road 1638 |
| 9.8 | b F-t3-044 | Inline interactive outputs: sandboxed generative-UI widgets with a host-validated prompt bridge | Recommended · M (was L) · W4 | Frontend + Model + Ingestion | S-F-t1-026 typed cards, S-F-t1-052 card registry and S-F-t1-014 viewer registry: a second rendering path for agent output. Title and roadmap only. | CONFLICT risk (one card and viewer registry) | b:945 (title); AppB 1861; road 1613 |
| 9.9 | b F-t3-045 | MCP Apps, elicitation forms, hosted artifact runtime on a dedicated user-content origin | Recommended · L · W4 | Frontend + Model + Application + Control | S-F-t1-014 viewer registry and S-F-t1-026 forms. Card proposes a new dedicated user-content origin; sibling ADR 9.3#6 prefers the existing sandboxed frame first (b:1688). | CONFLICT (new origin, section 5 C21) | b:957 (title); AppB 1862; road 1614; ADR b:1688 |
| 9.10 | a F-t2-042 | Agent web toolset: Quarry jobs as approval-gated tools with cost estimates | Recommended · L · W3 | Model + Ingestion | S-F-t1-019 run-bound grants and agent tools. Title and roadmap only. | REUSE | a:926 (title); AppB 1567; road 1320 |
| 9.11 | a F-t2-064 | External contract: OpenAPI, then an audience-bound MCP server | Nice to have (was Recommended) · XL · W4 | Ingestion + Control | S-F-t1-019 and S-F-t1-037 external exposure. Reverses a documented deferral; sibling ADR 9.3#7 (a:1391). | CONFLICT (external MCP decision) | a:1178 (section 6.6); AppB 1601; road 1346; ADR a:1391 |
| 9.12 | b F-t3-089 | Use Verevon from other agents and work tools: outward MCP endpoint, channel adapters | Nice to have · XL (was L) · W5 | Channel Plane | S ADR 10 (external surfaces) and S-F-t1-019. Channel Plane is docs-only (CLAUDE.md), so this cannot be a Space dependency. | DEP (unbuilt plane) | b:1314 (section 6.5); AppB 1907; road 1650 |
| 9.13 | b F-t3-063 | Loop guard, durable org hooks, universal tool-output handles | Recommended · L (was M) · W2 | Model | S-F-t1-045 guardrails: shell command policy and hooks. Title and roadmap only. | REUSE (hook engine) | b:1173 (title); AppB 1880; road 1573 |
| 9.14 | b F-t3-030 | Workspace file tools for Do mode: versioned read, grep, glob, governed edit | Recommended · M · W2 | Model | S-F-t1-015 room canvas and S-F-t1-046 files. Card says the hydrated Space workspace and CAS merge are real (quick win b:1674). | REUSE (Space workspace) | b:777 (title); AppB 1847; road 1569; quick win b:1674 |

### 2.10 Scheduled work, watches, automations, triggers (room automations S-F-t1-036 and webhooks S-F-t1-047; not in any WS01-WS20)

Space anchor: `S-F-t1-036` (S:882-894: room schedules, watches, calendar and event triggers; dependency at S:892), `S-F-t1-047` (S:1025-1037: inbound webhooks), S 9.4#8 (S:1653: schedule and watch decisions exist only for personal Spaces).

| # | ID | Title | Tier · Effort · Wave | Plane | Overlap with the Space plan (one line) | Verdict | Cite |
|---|---|---|---|---|---|---|---|
| 10.1 | b F-t3-018 | Scheduled runs become governed, tool-using Do runs with a stored approval contract | Priority · L · W2 | Model + Control + Application | S-F-t1-036 room schedules, fire outcomes and recipient consent. Card: owner signs a consent card; approved capabilities and expiry live in the Control-committed template (b:602) and are re-verified at each fire (b:610); off-surface approval goes through Application Plane delivery (b:610). | REUSE (Control-signed per-fire decision) + CONFLICT (off-surface consent versus the room consent flow) | b:597-612 (card 602, 604, 610); AppB 1835; road 1566; ADR b:1687; S:1653 |
| 10.2 | b F-t3-019 | Real Agents and Automations hubs: versioned definitions, run now and pause | Priority · XL · W3 | Model + Application + Frontend | S-F-t1-036 Work-tab schedule UI. Title and roadmap only. | DUP | b:613 (title); AppB 1836; road 1581 |
| 10.3 | b F-t3-047 | Event-triggered automations: normalised connector events wake budgeted runs | Recommended · XL (was L) · W4 | Model + Ingestion | S-F-t1-047 inbound webhooks that wake room agents and S-F-t1-036 calendar and event triggers: two ingress designs. Title and roadmap only. | CONFLICT (ingress owner, section 5 C9) | b:981 (title); AppB 1864; road 1616; ADR b:1687 |
| 10.4 | b F-t3-037 | Persistent goals with a token budget and completion judge | Recommended · XL (was L) · W3 | Model | S-F-t1-036 goals and S-F-t1-027 budgets. Title and roadmap only. | REUSE | b:861 (title); AppB 1854; road 1587 |
| 10.5 | a F-t2-018 | Make scrape, crawl and batch schedules execute (Temporal) | Priority · L · W2 | Ingestion | S-F-t1-036 schedules. Card: the Ingestion Plane owns schedules end to end and refuses schedule creation under ZDR (a:611, a:619); this is a second scheduler beside the Model Plane cron. | CONFLICT (schedule owner, section 5 C9) | a:606-621 (card 611, 619); AppB 1543; road 1292 |
| 10.6 | a F-t2-030 | Monitor change quality: fingerprint, diffs, tags, optional judge | Recommended · L · W3 | Ingestion + Model | S-F-t1-036 watches (process output wakes the room). Who owns a watch is undecided. Title and roadmap only. | CONFLICT (watch owner, section 5 C9) | a:782 (title); AppB 1555; road 1308 |
| 10.7 | a F-t2-031 | Tenant-scoped, SSRF-guarded outbound webhooks; completion-triggered follow-ups | Recommended · M · W3 | Ingestion | S-F-t1-047 inbound webhooks (opposite direction). Card: org scoping, SafeClient, idempotency keys and a chain-depth cap (a:803). Automation owner is open (ADR a:1393). | REUSE (SafeClient, idempotency, chain cap) + CONFLICT (automation owner) | a:794-805 (card 797, 801, 803); AppB 1556; road 1309; ADR a:1393 |
| 10.8 | a F-t2-061 | Agent routines: scheduled, heartbeat and event-triggered runs with per-fire grants | Nice to have (was Recommended) · XL · W4 | Model | S-F-t1-036 and S-F-t1-047. Card: cron ships only as a user-authored, Space-bound, draft-only lane in the capability-core sweeper; event triggers should come from Space Watch adapters (a:1172). | REUSE (existing sweeper; Watch adapters) | a:1172 (section 6.5); AppB 1599; road 1343; ADR a:1393 |
| 10.9 | b F-t3-071 | Coordinator-mode orchestration on Temporal with typed child results | Nice to have · M (was L) · W5 | Model + Frontend | S-F-t1-025 multi-agent rooms: background child work and delegation. Title and roadmap only. | REUSE | b:1276 (section 6.5); AppB 1889; road 1632 |

### 2.11 Anything else that touches the Space: sharing, artifacts, files, canvas, composer, quality gates, evals

Space anchor: `S-F-t1-057` (S:1155-1167), `S-F-t1-053` (S:1103-1115), `S-F-t1-015` (WS19, S:572-588), `S-F-t1-013` (WS14, S:536-552), `S-F-t1-032` (WS15, S:830-842), `S-F-t1-062` and `S-F-t1-024` (WS07/WS16), `S-F-t1-050` (S:1064-1076), `S-F-t1-067` (S:1272-1284).

| # | ID | Title | Tier · Effort · Wave | Plane | Overlap with the Space plan (one line) | Verdict | Cite |
|---|---|---|---|---|---|---|---|
| 11.1 | b F-t3-046 | Sharing, replay, export and import: governed snapshot links, org-only default | Recommended · XL (was L) · W4 | Model + Control | S-F-t1-057 Space-as-subject grants, S-F-t1-053 guest links, S-F-t1-039 permalinks. Card: refuse all three for ZDR threads; user-core's taxonomy lists document as the only ownable type (b:976); sibling ADR 9.3#6 puts grants in Control's taxonomy (b:1688). | CONFLICT (taxonomy and Space-as-subject, section 5 C18) + REUSE (Control taxonomy) | b:969-980 (card 972, 976, 978); AppB 1863; road 1615; ADR b:1688 |
| 11.2 | b F-t3-031 | Artifact canvas: server-side versions with restore, selection comments, collaborative documents | Recommended · XL · W3 | Model + Frontend + Application + Data | S-F-t1-015 room canvas (Data Plane wiki-store versions; canvas pages must be Space-owned, S:584-586). Card's version store is model-gateway `artifacts.rs` with lock_commit (b:796). Two version stores. | CONFLICT (version store owner, section 5 C7) | b:789-800 (card 792, 796, 798); AppB 1848; road 1584 |
| 11.3 | b F-t3-052 | Composer: atomic typed context tokens, per-thread drafts, thread references | Recommended · L · W3 | Frontend + Model | S-F-t1-032 composer (drafts, queue and steer, ref-based chips with pre-send preview). Two composers if built separately. Title and roadmap only. | DUP / CONFLICT (one composer) | b:1041 (title); AppB 1869; road 1594 |
| 11.4 | b F-t3-017 | Attachments v2: fail closed on unsupported media, durable reference-based uploads | Priority · L · W1 | Model + Data + Frontend | S-F-t1-013 presigned Data Plane documents with Space provenance, scan and typed refusals (WS14 is W2). The sibling's upload path lands first. Title and roadmap only. | REUSE (one upload path) | b:581 (title); AppB 1834; road 1541 |
| 11.5 | b F-t3-080 | Library of generated and uploaded files across chats, with org publishing on Data Plane documents | Nice to have · M · W5 | Data + Control | S-F-t1-013 Files view: a second file library. Title and roadmap only. | CONFLICT risk | b:1266 (section 6.5); AppB 1898; road 1641 |
| 11.6 | b F-t3-090 | Time-based media on demand: transcribe recordings into documents, meeting minutes | Nice to have · XL (was L) · W5 | Data + Frontend | S-F-t1-050 meeting intelligence and S ADR 7 (calls and meetings). ASR and capture owner must be decided in the same ADR (S:1636). | CONFLICT (capture owner) | b:1316 (section 6.5); AppB 1908; road 1651; S:1636 |
| 11.7 | b F-t3-038 | Deck pipeline with brand templates; org template library | Recommended · L · W3 | Model + Frontend + Data | S-F-t1-064 room templates and seed artifacts. Title and roadmap only. | INFO | b:873 (title); AppB 1855; road 1588 |
| 11.8 | b F-t3-066 | UI quality gates: remove Norwegian-only controls; i18n and design tokens in CI | Recommended · S (was M) · W1 | Frontend | S-F-t1-062 locale-correct grammar and reduced motion; one CI gate. | REUSE | b:1209 (title); AppB 1883; road 1548 |
| 11.9 | b F-t3-067 | Contract and architecture guardrails in CI: generated SPA types, plane-boundary gates, size ratchet | Recommended · L · W1 | Frontend | S-F-t1-024 build id, stale-tab reload and contract guard. Title and roadmap only. | REUSE | b:1221 (title); AppB 1884; road 1549 |
| 11.10 | b F-t3-078 | Feedback surfaces and limit notices: toasts, composer banners, connection pill | Nice to have · S (was M) · W5 | Frontend | S-F-t1-033 offline vs service-down banner and S-F-t1-062 failure screens. | DUP | b:1328 (section 6.6); AppB 1896; road 1639 |
| 11.11 | b F-t3-082 | Upgrade voice input; realtime delegation only if un-parked | Nice to have · L (was M) · W5 | Model + Frontend | S-F-t1-067 duplex voice for agents. Realtime delegation is parked by ruling; the Space plan must not assume it. | CONFLICT risk | b:1312 (section 6.5); AppB 1900; road 1643 |
| 11.12 | b F-t3-040 | Eval and replay release gates for prompts, tools and routing | Recommended · L · W3 | Model | S-F-t1-027 room-behaviour evals. Title and roadmap only. | DUP | b:897 (title); AppB 1857; road 1589 |
| 11.13 | a F-t2-044 | Hermetic browser and scrape evals in CI with injection and ZDR fixtures | Recommended · L · W3 | Ingestion + Model | S-F-t1-027 room-behaviour evals. Title and roadmap only. | DUP | a:950 (title); AppB 1569; road 1321 |
| 11.14 | a F-t2-037 | Document parsing depth: PDF classification, OCR routing, per-org parse cache | Recommended · L · W3 | Ingestion + Model | S-F-t1-046 extraction and S-F-t1-013 scanning: one parser and one OCR router. Card: imports-core already ships budgeted PDF and Office parsers (a:873). | REUSE (shared parser and OCR router) | a:866-877 (card 869, 873); AppB 1562; road 1315 |
| 11.15 | a F-t2-039 | Governed downloads and uploads: quarantined artifacts, Data Plane hand-off | Recommended · M · W3 | Ingestion + Data + Frontend | S-F-t1-013 attachment scan and S-F-t1-030 download safety. Card: scan hook before admitted types widen; Data Plane hand-off through the ingest client (a:897, a:899). | REUSE (scan hook, ingest client) | a:890-901 (card 893, 897, 899); AppB 1564; road 1317 |
| 11.16 | a F-t2-049 | URL and domain reputation gate; layered screening | Recommended · M · W3 | Ingestion + Model | S-F-t1-030 link safety and S-F-t1-052 link previews through Quarry. Title and roadmap only. | REUSE | a:1010 (title); AppB 1574; road 1325 |
| 11.17 | a F-t2-056 | Provider APIs and export rewrites before rendering (Google Docs, GitHub, Notion, transcripts) | Recommended · M · W4 | Ingestion | S-F-t1-052 link previews and S-F-t1-050 transcripts. Title and roadmap only. | INFO | a:1082 (title); AppB 1580; road 1338 |

## 3. ADR candidates that touch these pieces

### 3.1 Sibling ADRs (9.3) that touch the Space pieces

| Sibling ADR | Question (short) | Pieces | Space items and Space ADR it meets | Status in the sibling |
|---|---|---|---|---|
| a 9.3#1 (a:1385) | Web policy of record; tighten-only claims | 2.9, 6.5, 7.1 | S-F-t1-006, S-F-t1-027 (WS03); no Space ADR | Open; ship a Quarry-only ResolvedPolicy slice first (a:1385) |
| a 9.3#2 (a:1386) | Secret custody: integration-corev2 or a Control secret service | 2.12 | S-F-t1-044, S-F-t1-026 | Open; ship OTP first through the sensitive approval path |
| a 9.3#3 (a:1387) | Shared human and agent sessions; navigation policy | 6.2, 6.9 | WS20 (S-F-t1-020); S ADR 4 | Open; design grant binding, control lease and navigation policy together |
| a 9.3#4 (a:1388) | ZDR semantics per feature (caches, monitors, schedules, crawls, telemetry) | 3.1, 3.7, 10.5 | S-F-t1-005 (WS06), S-F-t1-036; S ADR 8 | Open; generate the matrix from route attributes |
| a 9.3#5 (a:1389) | Anti-bot posture and proxy escalation | 6.4 | S-F-t1-020 challenge hand-off | Open; confirm honest posture |
| a 9.3#6 (a:1390) | Script execution model (evaluate, action lists, code mode) | 8.7 | S-F-t1-019; S ADR 6 | Open; declarative lists fit the rule |
| a 9.3#7 (a:1391) | External MCP exposure (reverses a deferral) | 9.11 | S-F-t1-019, S-F-t1-037; S ADR 10 | Owner decision gated |
| a 9.3#8 (a:1392) | Run trace envelope and sharing; resource taxonomy | 1.8, 4.9 | WS02, WS09, S-F-t1-057; S ADR 1, ADR 2 | Extend model_plane.v1.Event; run sharing needs a taxonomy decision |
| a 9.3#9 (a:1393) | Automation owner (Application or Model Plane); per-fire grants | 10.5, 10.6, 10.7, 10.8 | S-F-t1-036, S-F-t1-047; S ADR 5 | Keep acquisition-only automation on Quarry schedules; agent routines wait |
| a 9.3#10 (a:1394) | Browser execution substrate (pool, containers or VMs, remote, extension) | 6.7, 6.8, 8.8 | S-F-t1-028; S ADR 6 | Per-page emulation now; CDP pool next; extension parked |
| b 9.3#1 (b:1683) | Sandbox computer backend and thread-scoped workspace | 8.1, 8.4 | S-F-t1-028; S ADR 6 | Phase 1 on bwrap; ADR before any microVM |
| b 9.3#2 (b:1684) | Computer-use scope; sandbox only; Endpoint Plane | 8.3 | S-F-t1-028; S ADR 6 | Sandbox only; Endpoint Plane only if parked |
| b 9.3#3 (b:1685) | Spaces as chat project; precedence; multi-author authority (OD19) | 1.1, 1.3, 1.6, 5.1 | S-F-t1-011, -012, -038; S ADR 3, ADR 5 | Spaces, not a second container; resolve the caller's Space role server-side; never a client-supplied space_ref |
| b 9.3#4 (b:1686) | Agent-browser domain policy owner; model-blind credentials (OD13) | 2.12, 6.9 | WS20, S-F-t1-044 | Admin allow and block lists in Control; Space narrows only; grants under the user's bearer |
| b 9.3#5 (b:1687) | Unattended authority for scheduled, goal and trigger runs (OD8, D10) | 10.1, 10.3 (approval classes in 2.2) | S-F-t1-036, S-F-t1-047; S ADR 5; S 9.4#5 (S:1650) | Contract belongs in Control, not a new Model Plane table |
| b 9.3#6 (b:1688) | Share model, public links, artifact hosting origin | 11.1, 9.9 | S-F-t1-057, -053, -015, -039; S ADR 10 | Org-only default; viewer-bound connector calls; no new origin before the existing sandboxed frame |
| b 9.3#7 (b:1689) | Learned and distributed behaviour: three-tier library, promotion gate, plugin pins | 5.5, 9.5 | S-F-t1-065, S-F-t1-046 | Build the staging gate before enabling the loop for real orgs |
| b 9.3#8 (b:1690) | Model entitlements and provider credential ownership | 7.7 | S-F-t1-006, S-F-t1-027, S-F-t1-044 | Control decides, Model enforces at admission; BYO keys in Integration Core custody |
| b 9.3#9 (b:1691) | LLM reviewers on the permission path | 2.6 | S-F-t1-045 | Only tighten-only, shadow first |
| b 9.3#10 (b:1692) | Connector content in shared Spaces; ZDR for connector and web reads | 9.4, 3.x (ZDR), 2.x | S-F-t1-037, -044, -046, S-F-t1-010 | Connector redaction before connector reads are proven live; web policy in both loops |

### 3.2 Space ADRs (S:1624-1642) and the sibling ADRs that meet them

| Space ADR | Sibling ADRs that meet it | Note |
|---|---|---|
| ADR 1 append authority, turn_origin (S:1624) | b 9.3#3 (b:1685) | Origin naming conflict (section 5 C4) |
| ADR 2 realtime tier under ZDR (S:1626) | a 9.3#8 (a:1392) | Envelope: extend model_plane.v1.Event |
| ADR 3 agent memory and labels (S:1628) | none as ADR; items 5.1, 5.4 | Containment slice is a Space and sibling item (section 5 C13) |
| ADR 4 personal agents as service subjects (S:1630) | none | Items: 10.1 touches owner-signed consent |
| ADR 5 delegation and whose authority (S:1632) | a 9.3#9 (a:1393); b 9.3#5 (b:1687) | Per-fire re-verification |
| ADR 6 network boundary, persistence, local executor (S:1634) | a 9.3#6, #10 (a:1390, a:1394); b 9.3#1, #2 (b:1683, b:1684) | Enforcement point (section 5 C8); plane registration (C20) |
| ADR 7 calls and meetings (S:1636) | none as ADR; item 11.6 | ASR and capture owner |
| ADR 8 retention, legal hold, ZDR precedence (S:1638) | a 9.3#4 (a:1388); item 3.11 | Ordering of hold, retention and refusal |
| ADR 9 message search index placement (S:1640) | item 1.1; b 9.4#5 (b:1700) | Same design: index in Session Core, ZDR excluded |
| ADR 10 external surfaces and guests (S:1642) | a 9.3#7 (a:1391); b 9.3#6 (b:1688) | Items: 9.11, 9.12 |

## 4. Roadmap wave ordering for the pieces

Each cell lists the row ids from section 2 and the sibling item, in its roadmap wave (road lines in section 0). "--" means no row in that wave.

| Piece | Sib | W1 | W2 | W3 | W4 | W5 |
|---|---|---|---|---|---|---|
| 2.1 Session Core authority | b | 1.2 F-t3-016; 1.4 F-t3-002; 1.6 F-t3-001 | 1.1 F-t3-012; 1.3 F-t3-013; 1.5 F-t3-049 | -- | 1.12 F-t3-064 | 1.7 F-t3-069 |
| 2.1 Session Core authority | a | 1.11 F-t2-006 | 1.8 F-t2-014 | 1.9 F-t2-035; 1.10 F-t2-038 | -- | -- |
| 2.2 Approvals and grants | b | 2.2 F-t3-003 | 2.1 F-t3-006; 2.3 F-t3-011; 2.4 F-t3-032; 2.7 F-t3-033 | -- | -- | 2.5 F-t3-074; 2.6 F-t3-097 |
| 2.2 Approvals and grants | a | 2.8 F-t2-001 | -- | 2.9 F-t2-032; 2.10 F-t2-033; 2.11 F-t2-036; 2.12 F-t2-024 | -- | 2.13 F-t2-076 |
| 2.3 ZDR and refusals | b | 3.8 F-t3-005; 3.9 F-t3-004 | -- | 3.11 F-t3-062; 3.12 F-t3-041 | -- | 3.10 F-t3-075 |
| 2.3 ZDR and refusals | a | 3.1 F-t2-003; 3.2 F-t2-004; 3.3 F-t2-005 | 3.4 F-t2-020; 3.5 F-t2-021 | 3.6 F-t2-050; 3.7 F-t2-048 | -- | -- |
| 2.4 Run visibility | b | 4.6 F-t3-021 | 4.1 F-t3-015 | 4.3 F-t3-026; 4.4 F-t3-048; 4.5 F-t3-034 | 4.2 F-t3-035; 4.7 F-t3-050 | -- |
| 2.4 Run visibility | a | -- | -- | -- | -- | 4.8 F-t2-071; 4.9 F-t2-072 |
| 2.5 Memory | b | 5.2 F-t3-020; 5.5 F-t3-007 | -- | 5.1 F-t3-060; 5.3 F-t3-061 | -- | 5.6 F-t3-094; 5.7 F-t3-093 |
| 2.5 Memory | a | -- | 5.4 F-t2-045 | -- | -- | -- |
| 2.6 Browser | b | -- | 6.9 F-t3-008 | -- | -- | 6.10 F-t3-081 |
| 2.6 Browser | a | 6.4 F-t2-027; 6.6 F-t2-007; phase 1 of 6.1 F-t2-013 | 6.1 F-t2-013; 6.2 F-t2-011; 6.3 F-t2-012; 6.5 F-t2-022 | -- | 6.7 F-t2-059; 6.8 F-t2-060 | -- |
| 2.7 Cost and telemetry | b | 7.6 F-t3-022; 7.7 F-t3-023 | 7.5 F-t3-028 | 7.8 F-t3-068 | -- | 7.9 F-t3-079; 7.10 F-t3-083 |
| 2.7 Cost and telemetry | a | -- | 7.1 F-t2-008; 7.2 F-t2-029 | 7.3 F-t2-046 | 7.4 F-t2-062 | 7.11 F-t2-073 |
| 2.8 Sandbox and egress | b | -- | 8.1 F-t3-009 | 8.2 F-t3-043 | 8.3 F-t3-024 | 8.4 F-t3-085; 8.5 F-t3-084 |
| 2.8 Sandbox and egress | a | -- | -- | -- | 8.6 F-t2-058; 8.7 F-t2-063 | 8.8 F-t2-077 |
| 2.9 Registry, connectors, plugins | b | -- | 9.1 F-t3-025; 9.4 F-t3-010; 9.13 F-t3-063; 9.14 F-t3-030 | 9.2 F-t3-056; 9.6 F-t3-058 | 9.3 F-t3-057; 9.5 F-t3-059; 9.8 F-t3-044; 9.9 F-t3-045 | 9.7 F-t3-077; 9.12 F-t3-089 |
| 2.9 Registry, connectors, plugins | a | -- | -- | 9.10 F-t2-042 | 9.11 F-t2-064 | -- |
| 2.10 Automation | b | -- | 10.1 F-t3-018 | 10.2 F-t3-019; 10.4 F-t3-037 | 10.3 F-t3-047 | 10.9 F-t3-071 |
| 2.10 Automation | a | -- | 10.5 F-t2-018 | 10.6 F-t2-030; 10.7 F-t2-031 | 10.8 F-t2-061 | -- |
| 2.11 Other | b | 11.4 F-t3-017; 11.8 F-t3-066; 11.9 F-t3-067 | -- | 11.2 F-t3-031; 11.3 F-t3-052; 11.7 F-t3-038; 11.12 F-t3-040 | 11.1 F-t3-046 | 11.5 F-t3-080; 11.6 F-t3-090; 11.10 F-t3-078; 11.11 F-t3-082 |
| 2.11 Other | a | -- | -- | 11.13 F-t2-044; 11.14 F-t2-037; 11.15 F-t2-039; 11.16 F-t2-049 | 11.17 F-t2-056 | -- |

Wave comparison with the Space plan (Space WS waves from `workstreams.md`): WS02 is W1 but its typed event contract is set by sibling W2 items (inversion, section 5 O1). WS03 is W1 but the policy-of-record ADR is needed before W2 (inversion, O2). WS17 is W3 while the sibling containment slice is W1-level (O3). WS10 (W2), WS18 (W3) and WS20 (W3) all depend on sibling items that land earlier, which is consistent. S-F-t1-036 and S-F-t1-047 are not in any WS and depend on sibling W2 items (O7).

## 5. Conflicts and ordering risks

Each item names the decision owner and the Space workstream that must wait for it. The ids in brackets point to section 2 rows.

### 5.1 Conflicts (owner decision before the affected Space work starts)

- **C1 Client-supplied `space_ref` on browser runs** [6.9, 1.3]. Space adds `space_ref` to the browser run start request, authorised by a Control decision bound to the room audience hash and revision (S:665), and plumbs it into `ExecuteStepRequest.space_id` (S:673). Sibling b requires the Space to be resolved server-side through resolve_space_role, never client-supplied (b:546, b:1685). Decision: the server resolves the room from the thread or run; the client sends a thread id only. Blocks WS20.
- **C2 Queue and steer** [1.2, 1.10]. Space WS15 (S-F-t1-032, S:830-842) wants queue and steer and an editable queue; WS09 (S-F-t1-040, S:938-944) wants member steering by policy. Sibling b F-t3-016 (W1) drops the editable queue and split send button unless the owner overrides the round-boundary ruling (b:1252). Decision: owner ruling on mid-run input. Blocks WS09 and WS15.
- **C3 Grant scope vocabulary** [2.1, 2.9, 2.12]. Space S-F-t1-010 has no once, session or always scopes yet (S:66). S-F-t1-044 uses once, session, standing (S:986-998). b F-t3-006 uses once, this thread, always as a reviewed rule (b:426). a F-t2-032 uses turn, conversation, always, with any deny beating any approval (a:809). Decision: one scope enum owned by Control, one deny-precedence rule. Blocks WS10.
- **C4 Origin vocabulary** [1.1, 1.8, 1.12, 10.1]. Space ADR 1 turn_origin: direct, ambient, automation (S:1624) and decisions carry turn_origin (S:491). b F-t3-018 origin: chat, schedule, trigger (b:602). b F-t3-064: origin and parent id on every run, do not widen the six-value thread-origin CHECK (b:1192). a F-t2-014: a run origin enum (a:547). Decision: one origin enum, stored once, carried by the Model Plane envelope. Blocks WS01, WS02, WS09.
- **C5 Policy registry of record** [2.4, 6.5, 7.7]. Space WS03 (S-F-t1-006, W1) puts the effect-policy admin in user-core (S:422). b F-t3-032 (W2) proposes a Control-signed tighten-only registry with locks (b:804). a F-t2-022 (W2) and ADR a:1385 leave the policy of record open (org-core claims or capability-core grants). Decision: one policy of record before WS03 phase 2. Blocks WS03 phase 2.
- **C6 Run-action authority table** [9.1]. S-F-t1-019 (WS18) needs Control to mint per-run authority for Space actions; runActionTarget accepts only tickets.create (S:652; b:724). b F-t3-025 proposes one per-action table keyed by schema hash. Decision: extend the one table; no Space-only table. Blocks WS18.
- **C7 Canvas and version store** [11.2]. S-F-t1-015 (WS19, W3) uses Data Plane wiki-store versions with Space-owned canvas pages (S:576, S:584-586). b F-t3-031 (W3) keeps artifact versions in model-gateway `artifacts.rs` with lock_commit (b:796). Decision: one version store. Blocks WS19.
- **C8 Egress enforcement point and tier vocabulary** [6.7, 6.8, 8.2, 8.6]. S ADR 6 (S:1634): Quarry executes, sandboxes have no direct network. b F-t3-043 (W3): closed tier enum in the Control-signed Space capability decision, enforced by netns or firewall reusing Quarry's dns_guard (b:940). a F-t2-058 (W4): country and locale egress tiers (a:1094). a F-t2-059 and F-t2-060 (W4): containment proof for remote and isolated browsers (a:1106-1129). Decision: one enforcement point and one tier vocabulary. Blocks the S-F-t1-021 egress slice and any remote browser backend.
- **C9 Schedule and watch owner** [10.1, 10.3, 10.5, 10.6, 10.7, 10.8]. S-F-t1-036 needs room schedules and watches with per-fire Control re-authorisation (S:882-894). a F-t2-018 (W2): Ingestion owns schedules end to end (a:619). a F-t2-061 (W4): cron is in the capability-core sweeper and event triggers should come from Space Watch adapters (a:1172). b F-t3-018 (W2): Control-committed template plus Model Plane supervision (b:602, b:610). a ADR 9.3#9 (a:1393) is open. Decision: one owner per schedule kind (acquisition, agent run, watch). Blocks S-F-t1-036.
- **C10 Room-history recall** [1.1]. b F-t3-012 keeps recall owner-scoped because recall is a prompt-injection channel (b:530). S-F-t1-038 gives agents search_space_history under the asker's authority (S:908-920). Decision: whether agents may search room history, and which Control decision authorises it. Blocks S-F-t1-038.
- **C11 Message versions and revisions** [1.1]. b F-t3-012 says versions are client-only because an earlier persisted design was rejected in review (b:528; apps/Model Plane/docs/CHAT_RESUME_AND_VERSIONS_SPEC.md). S-F-t1-017 (WS12) uses server-side revision entries (S:606-622). Decision: one revision model in Session Core, reconciled with the rejected design. Blocks WS12.
- **C12 Memory label schema** [5.1]. b F-t3-060: scope kind and id (user, space, project, agent) with an isolation flag (b:1140). S-F-t1-012 (WS17): space_ref, audience hash and revision on agent_memory rows (S:533). Decision: one schema in the S ADR 3 slice. Blocks WS17.
- **C13 Memory containment has two owners** [5.1, 5.2]. b F-t3-060 step (1) (b:1144) is the S ADR 3 containment slice (S:1628). Decision: owned by WS17 and merged; no second implementation. Blocks WS17.
- **C14 Two composers** [11.3]. S-F-t1-032 (WS15, W2) and b F-t3-052 (W3) each define drafts and context chips. Decision: one composer and one draft store. Blocks WS15.
- **C15 Audit and SIEM pipeline (three designs)** [3.9, 2.11, 7.11]. S-F-t1-031 (WS05): org audit trail with Space decisions and agent invocations, SIEM and usage export (S:817-829). b F-t3-004 (W1): hash chain, compliance API and SIEM fan-out in audit-core with content-free rows (b:390-398). a F-t2-036 (W3): AuthzDecision event on verevon.audit.v2 (a:861). a F-t2-073 (W5): SIEM export on top of a pull API (a:1160). Decision: audit-core owns export; one event shape. Blocks WS05 phase 2.
- **C16 ZDR matrix and posture carrier** [3.1, 3.2]. S-F-t1-005 (WS06, W1): commit the untracked zdr_refusal.rs and a matrix test (S:1289). a F-t2-003 (W1): matrix from route attributes in quarry-core (a:357). a F-t2-004 (W1): no ZDR header and no NATS header; posture from `Claims::effective_zdr_mode` (a:383). Decision: one matrix format and one posture source. Blocks WS06.
- **C17 Run-state vocabulary and content-free wake** [4.1]. b F-t3-015 (W2): run-state enum working, needs_approval, needs_input, plan_ready, completed, failed, archived (b:570). Space S-F-t1-056 (needs-input), S-F-t1-009 (working aggregate), S-F-t1-035 (content-free Web Push; a Control carve-out for ZDR wake, S:877-879). Decision: one enum; the wake contract is content-free on every thread. Blocks WS11 and WS16 sidebar.
- **C18 Resource taxonomy and sharing** [4.9, 11.1]. Space S-F-t1-057 (WS14, W2) wants Space-as-subject grants (S:1155-1167). user-core's taxonomy classes quarry_run as team_shared and rejects per-user grants (a:1159; apps/Control Plane/user-core/internal/authztaxonomy/taxonomy.go:91-101). The taxonomy lists document as the only ownable type (b:976). Sibling ADR a:1392 says run sharing needs a taxonomy decision first. Decision: taxonomy first, then sharing. Blocks the WS14 grants slice and any run sharing.
- **C19 Outward actions and connector content** [9.2, 9.4]. S-F-t1-044 (draft-first sends; Control grants once, session, standing; S:986-998). b F-t3-056 (W3): editable write confirmation with policy levels (b:1089). b F-t3-010 (W2): connector reads must be viewer-bound; ADR b:1692 asks how personal mailbox content in a shared Space stays from other members. Decision: one outward-action confirmation policy; viewer-bound connector calls. Blocks S-F-t1-037 and S-F-t1-044.
- **C20 Endpoint Plane** [8.3]. b F-t3-024 names "Endpoint Plane" as owner (b:1611, b:1841), and b ADR 9.3#2 (b:1684) offers sandbox-only or sandbox plus Endpoint Plane. The folder `apps/Endpoint Plane/` exists and is COMMITTED (30 tracked files), but CLAUDE.md lists only the focused planes, Support Plane, and the docs-only Channel Plane. Space ADR 6 (S:1634) requires a registered plane first. Decision: register the plane or drop the name. Blocks any computer work beyond the sandbox.
- **C21 New user-content origin** [9.9, 9.8]. b F-t3-045 proposes a dedicated user-content origin for MCP Apps and hosted artifacts (b:957). b ADR 9.3#6 prefers the existing sandboxed frame first (b:1688). a F-t2-071 removes the permissive iframe (a:1155). Decision: no new origin until the Space viewer registry is decided (S-F-t1-014, S:553-568). Blocks WS07 viewer work.
- **C22 Trajectory capture under ZDR** [4.5]. b F-t3-034 captures exact model-visible requests (b:825). Session Core refuses durable writes for ZDR callers (S:1624 context). Decision: refuse capture for ZDR threads explicitly. Blocks any Space run-trace UI that persists requests.
- **C23 Letta mirror** [5.4]. Only a:969 and a:1836 mention Letta; the 03 file has no Letta reference. Space ADR 3 says Letta must receive the same filter (S:1628). Decision: the memory owner suppresses the mirror for tainted and room-origin writes. Blocks WS17.

### 5.2 Ordering risks (Space wave versus sibling wave)

- **O1** WS02 (realtime, W1) defines a typed seq-numbered projection (S-F-t1-003), while the sibling envelope (a F-t2-014, W2) and lifecycle taxonomy (b F-t3-049, W2) are later. Mitigation: WS02 adopts `model_plane.v1.Event` and the F-t3-049 event names as its contract in W1.
- **O2** WS03 phase 1 (effect-policy admin, W1) lands before the policy-of-record ADR that a:1400 orders before W2 items. Mitigation: phase 1 is preset and read-only; no write path until C5 is decided.
- **O3** WS17 (memory, W3) comes after the sibling containment work (b F-t3-020, W1; S ADR 3 slice). Mitigation: move the containment slice into W1.
- **O4** WS20 (room browser, W3) depends on a F-t2-001 (W1), a F-t2-013 phase 1 (W1), a F-t2-007 (W1), a F-t2-027 (W1) and b F-t3-008 (W2). Consistent; the Space plan must not start WS20 slices before those land.
- **O5** WS18 (agent self-API, W3) depends on b F-t3-003 (W1) and b F-t3-025 (W2). Consistent.
- **O6** WS10 (approvals, W2) depends on b F-t3-002 (W1) and b F-t3-006 (W2). Consistent.
- **O7** S-F-t1-036 (room automations) and S-F-t1-047 (webhooks) are in no WS (workstreams.md lists WS01-WS20 only). Their sibling dependencies a F-t2-018 and b F-t3-018 are W2. Mitigation: add an explicit WS (or a W3 epic) and a dependency on C9.
- **O8** WS16 (sidebar, W2) and b F-t3-015 (W2) both rebuild the sidebar around run states. Mitigation: one slice, owned by WS16 (C17).
- **O9** WS14 (attachments, W2) follows b F-t3-017 (W1, durable reference-based uploads). Mitigation: reuse the upload path instead of building a second one.
- **O10** WS13 search (W2) and b F-t3-012 (W2) both need session-core migrations. Next free number is 0046 (0044 and 0045 are UNCOMMITTED-NEW; 0043 is the highest tracked). Mitigation: reserve 0046 for one of them and sequence the other.
- **O11** The workstreams.md touchpoint list (cross-session section) omits WS03, WS05, WS07, WS14, WS16, WS19 and the scheduled-work items above, all of which have overlap rows here. Mitigation: add them to the Space plan's cross-session section.

## 6. Coverage by workstream (overlap rows per WS)

| WS | Space items | Sibling rows touching it (row id: short verdict) |
|---|---|---|
| WS01 | F-t1-001, F-t1-033 | 1.2 (REUSE receipts, CONFLICT queue); 1.1 (REUSE index and tree); 1.3 (CONFLICT space_ref) |
| WS02 | F-t1-003 | 1.5 (REUSE taxonomy); 1.8 (REUSE envelope); 1.9 (REUSE no second state machine); O1 |
| WS03 | F-t1-006 | 2.4 (CONFLICT registry); 6.5 (CONFLICT policy of record); 7.7, 7.8 (REUSE); C5 |
| WS04 | F-t1-022, -048, -049 | none found (INFO only) |
| WS05 | F-t1-031, F-t1-023 | 3.9 (REUSE audit-core); 2.11 (REUSE audit shape); 7.11 (DUP); C15 |
| WS06 | F-t1-005, F-t1-021 | 3.1, 3.2 (REUSE and CONFLICT risk); 3.8 (DEP); 8.2, 8.6 (CONFLICT); C8, C16 |
| WS07 | F-t1-024, -030, -063, -014 | 4.8 (CONFLICT risk, iframe); 11.8, 11.9 (REUSE); 4.9 (CONFLICT taxonomy); C21 |
| WS08 | F-t1-002, F-t1-007 | none found |
| WS09 | F-t1-004, F-t1-040 | 1.2, 1.10 (CONFLICT queue, DUP steer); 4.1 (CONFLICT vocabulary); 4.3 (REUSE takeover); C2, C17 |
| WS10 | F-t1-010 | 2.1 (CONFLICT scopes); 2.2, 2.3, 2.7 (REUSE); 2.9 (CONFLICT scopes); 2.11 (REUSE); C3 |
| WS11 | F-t1-008, -009, -042 | 4.1 (CONFLICT vocabulary); 2.10 (DUP approval card); C17 |
| WS12 | F-t1-016, -017 | 1.1 (CONFLICT revisions); 1.7 (DEP); C11 |
| WS13 | F-t1-018, -039, -041 | 1.1 (REUSE index, ZDR exclusion); 9.7 (DUP command surface); C10 |
| WS14 | F-t1-013, -057 | 11.4 (REUSE upload path); 11.15 (REUSE scan hook); 4.9, 11.1 (CONFLICT taxonomy); C18 |
| WS15 | F-t1-032, -034, -052 | 1.2 (CONFLICT queue); 4.6 (DUP timeline); 11.3 (DUP composer); C2, C14 |
| WS16 | F-t1-062, F-t1-055 | 11.8 (REUSE gates); 4.1 (sidebar, O8); O8 |
| WS17 | F-t1-011, -012 | 5.1 (CONFLICT label; REUSE containment); 5.2 (REUSE); 5.4 (REUSE taint); 5.3 (CONFLICT risk); C12, C13, C23 |
| WS18 | F-t1-019 | 9.1 (REUSE table, CONFLICT Space-only table); 9.10 (REUSE); 9.11 (CONFLICT external MCP); C6 |
| WS19 | F-t1-015 | 11.2 (CONFLICT version store); 9.14 (REUSE workspace); C7 |
| WS20 | F-t1-020 | 6.1 (DEP); 6.5 (CONFLICT policy); 6.9 (REUSE, CONFLICT space_ref); 6.4, 6.6 (DEP/REUSE); 4.3 (REUSE); C1, C8 |
| Scheduled work (no WS) | F-t1-036, F-t1-047 | 10.1 (REUSE, CONFLICT consent); 10.3, 10.5-10.8 (CONFLICT owner); C9, O7 |

## 7. Not checked, unverified, or limits

- Rows whose cite says "title", "title/road" or "road" rely on the Appendix B title, the roadmap dependency or the ADR text. Their card bodies were not read, so their verdicts are not checked against the design text.
- Space item bodies were read only at title level, plus the targeted lines cited with `S:`, for F-t1-010, -013, -014, -015, -018, -019, -020, -027, -031, -032, -035, -036, -040, -041, -044, -045, -046, -047, -056, -057, -065.
- `apps/Model Plane/proto/model_plane/v1/events.proto:8-60` (cited by a:553) was checked at line 8 only, an import line. The envelope range is UNVERIFIED here.
- `space_capability_decision.go:100-175` was checked at lines 100-101 and 158. The claim that the claim grants `space:egress` without entitlement is UNVERIFIED here.
- The Space plan's workstream files (`workstreams.md`) were read as written; no WS slice design was checked.
- OD ids (OD8, OD13, OD19) point to `apps/Frontend Plane/verevonv3/apps/verevon-web/plans/system-audits/CHAT_GAP_CLOSURE_PLAN_2026-10-01.md`, which is COMMITTED but was not opened.
- `zdr_refusal.rs` and `retention.rs` (UNCOMMITTED-NEW) were not read.
- Reference-side verification (sibling Appendix C) was not read; it does not change any overlap verdict here.
- No quoted passage exceeds the copyright limit; sibling and Space statements are paraphrased.

## 8. Path states (cited paths, four-state classification)

States from `.plan-work/live-tree-status.txt` (quote-aware) and `.plan-work/live-tree-tracked-files.txt`. All 73 paths exist in the live tree (`C:/dev/CoresSystem`, read-only). Counts: COMMITTED 58, MODIFIED-UNCOMMITTED 11, UNCOMMITTED-NEW 4, MISSING 0.

| State | Path |
|---|---|
| MODIFIED-UNCOMMITTED | apps/Model Plane/rust/services/session-core/src/grpc.rs |
| MODIFIED-UNCOMMITTED | apps/Model Plane/rust/services/session-core/src/dreaming.rs |
| MODIFIED-UNCOMMITTED | apps/Model Plane/rust/services/model-gateway/src/sse.rs |
| MODIFIED-UNCOMMITTED | apps/Model Plane/rust/services/model-gateway/src/sse_events.rs |
| MODIFIED-UNCOMMITTED | apps/Model Plane/rust/services/model-gateway/src/browser_run.rs |
| MODIFIED-UNCOMMITTED | apps/Model Plane/rust/services/model-gateway/src/auth.rs |
| MODIFIED-UNCOMMITTED | apps/Model Plane/rust/services/execution-core/src/grpc.rs |
| MODIFIED-UNCOMMITTED | apps/Model Plane/rust/services/execution-core/src/runtime_loop/agent.rs |
| MODIFIED-UNCOMMITTED | apps/Model Plane/rust/services/execution-core/src/runtime_loop/mod.rs |
| MODIFIED-UNCOMMITTED | apps/Model Plane/rust/crates/mp-contracts/src/dataplane_posture.rs |
| MODIFIED-UNCOMMITTED | apps/Ingestion Plane/Quarry-v2/crates/quarry-edge/src/routes.rs |
| UNCOMMITTED-NEW | apps/Model Plane/rust/crates/mp-contracts/src/zdr_refusal.rs |
| UNCOMMITTED-NEW | apps/Model Plane/rust/crates/mp-contracts/src/retention.rs |
| UNCOMMITTED-NEW | apps/Model Plane/rust/services/session-core/migrations/0044_run_retention_receipt.sql |
| UNCOMMITTED-NEW | apps/Model Plane/rust/services/session-core/migrations/0045_zdr_waived_runs_org_index.sql |
| COMMITTED | apps/Model Plane/rust/services/session-core/src/audit_publisher.rs |
| COMMITTED | apps/Model Plane/rust/services/session-core/migrations/0004_tasks_cron_hooks_skills_memory.sql |
| COMMITTED | apps/Model Plane/rust/services/session-core/migrations/0039_memory_user_control.sql |
| COMMITTED | apps/Model Plane/rust/services/session-core/migrations/0043_audit_outbox_requeue.sql |
| COMMITTED | apps/Model Plane/rust/services/execution-core/src/browser_agent.rs |
| COMMITTED | apps/Model Plane/rust/services/execution-core/src/browser_agent/browser_risk.rs |
| COMMITTED | apps/Model Plane/rust/services/execution-core/src/capability_policy.rs |
| COMMITTED | apps/Model Plane/rust/services/execution-core/src/permission/mod.rs |
| COMMITTED | apps/Model Plane/rust/services/execution-core/src/policy.rs |
| COMMITTED | apps/Model Plane/rust/services/execution-core/src/quarry_agent.rs |
| COMMITTED | apps/Model Plane/rust/services/execution-core/src/sandbox.rs |
| COMMITTED | apps/Model Plane/rust/services/model-gateway/src/artifacts.rs |
| COMMITTED | apps/Model Plane/rust/services/model-gateway/src/cancel_registry.rs |
| COMMITTED | apps/Model Plane/rust/services/model-gateway/src/idempotency_registry.rs |
| COMMITTED | apps/Model Plane/rust/services/model-gateway/src/queued_input.rs |
| COMMITTED | apps/Model Plane/proto/model_plane/v1/events.proto |
| COMMITTED | apps/Model Plane/proto/model_plane/v1/orchestration.proto |
| COMMITTED | apps/Model Plane/proto/model_plane/v1/execution.proto |
| COMMITTED | apps/Model Plane/go/services/capability-core/internal/cron/sweeper.go |
| COMMITTED | apps/Model Plane/go/services/capability-core/internal/cron/control_authorizer.go |
| COMMITTED | apps/Model Plane/go/services/capability-core/internal/policy/engine.go |
| COMMITTED | apps/Model Plane/go/services/capability-core/internal/runwatch/consumer.go |
| COMMITTED | apps/Model Plane/go/services/orchestrator-core/cmd/workflows/interactive_run.go |
| COMMITTED | apps/Model Plane/go/services/sandbox-manager/internal/lease/lease.go |
| COMMITTED | apps/Model Plane/go/services/browser-broker/internal/grant/grant.go |
| COMMITTED | apps/Model Plane/go/services/browser-broker/cmd/main.go |
| COMMITTED | apps/Model Plane/go/services/cost-core/internal/ledger/ledger.go |
| COMMITTED | apps/Model Plane/docs/decisions/ledger.md |
| COMMITTED | apps/Model Plane/docs/postmortem/0002-session-audit-trail-never-delivered.md |
| COMMITTED | apps/Model Plane/docs/CHAT_RESUME_AND_VERSIONS_SPEC.md |
| COMMITTED | apps/AUTHORED_INSTRUCTIONS_ADR_2026-08-19.md |
| COMMITTED | apps/Control Plane/user-core/internal/spaces/personal_thread_decision.go |
| COMMITTED | apps/Control Plane/user-core/internal/spaces/run_action_decision.go |
| COMMITTED | apps/Control Plane/user-core/internal/spaces/space_capability_decision.go |
| COMMITTED | apps/Control Plane/user-core/internal/authztaxonomy/taxonomy.go |
| COMMITTED | apps/Control Plane/user-core/internal/authztaxonomy/resource_taxonomy.json |
| COMMITTED | apps/Control Plane/user-core/internal/http/handlers.go |
| COMMITTED | apps/Control Plane/audit-core/internal/api/api.go |
| COMMITTED | apps/Control Plane/org-core/internal/org/types.go |
| COMMITTED | apps/Frontend Plane/verevonv3/apps/gateway/src/domains/browser.rs |
| COMMITTED | apps/Frontend Plane/verevonv3/apps/gateway/src/domains/search.rs |
| COMMITTED | apps/Frontend Plane/verevonv3/apps/gateway/src/domains/chat/history.rs |
| COMMITTED | apps/Frontend Plane/verevonv3/apps/gateway/src/domains/spaces.rs |
| COMMITTED | apps/Frontend Plane/verevonv3/src/shared/actions/model-eligibility.ts |
| COMMITTED | apps/Frontend Plane/verevonv3/src/shared/actions/types.ts |
| COMMITTED | apps/Frontend Plane/verevonv3/src/features/chat/lib/chat-organization.ts |
| COMMITTED | apps/Frontend Plane/verevonv3/docs/space-defenition.md |
| COMMITTED | apps/Frontend Plane/verevonv3/apps/verevon-web/plans/system-audits/CHAT_GAP_CLOSURE_PLAN_2026-10-01.md |
| COMMITTED | apps/Ingestion Plane/Quarry-v2/crates/quarry-edge/src/agent_routes.rs |
| COMMITTED | apps/Ingestion Plane/Quarry-v2/crates/quarry-edge/src/auth.rs |
| COMMITTED | apps/Ingestion Plane/Quarry-v2/crates/quarry-edge/src/telemetry.rs |
| COMMITTED | apps/Ingestion Plane/Quarry-v2/crates/quarry-browser/src/chromiumoxide.rs |
| COMMITTED | apps/Ingestion Plane/Quarry-v2/crates/quarry-runtime/src/step_receipts.rs |
| COMMITTED | apps/Ingestion Plane/Quarry-v2/services/quarry-control/internal/resources/schedule_wire.go |
| COMMITTED | apps/Ingestion Plane/Quarry-v2/services/quarry-orchestrator/internal/schedules/schedules.go |
| COMMITTED | apps/Ingestion Plane/Quarry-v2/docs/OSS_PARITY_BACKLOG.md |
| COMMITTED | apps/Ingestion Plane/integration-corev2/internal/api/connection_access.go |
| COMMITTED | apps/Endpoint Plane/Cargo.toml |

## 9. Line checks against the live tree (read-only)

Each cited line was read in the live file. "match" = the cited line holds the cited symbol or statement; "region" = the cited range holds the described code but the exact line was not confirmed; "drift" would mean a different line. No drift was found.

| Cited | Live line (short) | Result | State |
|---|---|---|---|
| S:4504 owner check on append (session-core grpc.rs) | 4504 `authorize_thread_owner(... OwnerIntent::Mutate)` in `append_message` (fn at 4495) | match | MODIFIED-UNCOMMITTED |
| user-core run_action_decision.go:340-347 | 340 `func runActionTarget(actionID string) (string, bool) {` | match (region) | COMMITTED |
| gateway browser.rs:1235 (`session:{id}` grant) | 1235 `"grant_id": format!("session:{session_id}"),` | match | COMMITTED |
| quarry-edge agent_routes.rs:2222-2310 (step handler) | 2228 `pub async fn step(` | match (sibling cites 2227-2294) | COMMITTED |
| gateway search.rs:285 (store with no ZDR check) | 285 `state.cache.store(&key, &cached_payload).await;` | match | COMMITTED |
| model-gateway sse.rs:1855 (authored instructions composed) | 1855 `prompt.authored_instructions = authored_instructions_message(...)` | match | MODIFIED-UNCOMMITTED |
| model-gateway sse.rs:1443-1520 (agentic return) | 1443 comment `chat-parity Phase 3 — agentic run` | region | MODIFIED-UNCOMMITTED |
| execution-core grpc.rs:498-530 (scheduled step, no tools) | 498 `fn scheduled_inference_request(` | match | MODIFIED-UNCOMMITTED |
| session-core dreaming.rs:1263-1285 (pending messages across threads) | 1263 `async fn load_pending_messages(` | match | MODIFIED-UNCOMMITTED |
| model-gateway browser_run.rs:808-826 (AcquireGrant in test double) | 808 test-module import of AcquireGrantRequest | region | MODIFIED-UNCOMMITTED |
| execution-core runtime_loop/agent.rs:4204-4212 (browser.observe advertised) | 4204 `name: "browser.observe".to_owned(),` | match | MODIFIED-UNCOMMITTED |
| quarry-edge routes.rs:563-588 (durable refusal under ZDR) | 563 `fn refuse_durable_job_under_zdr(` | match | MODIFIED-UNCOMMITTED |
| user-core taxonomy.go:91-101 (quarry_run team_shared) | 91 `func ValidateUserGrant(resourceType string) error {` | region | COMMITTED |
| cost-core ledger.go:40-50 (ledger dimensions) | 44 comment: tenant, actor, run, request, model, producer | match (at 44) | COMMITTED |
| gateway model-eligibility.ts:126-140 (8 of 168 actions) | 126 `MODEL_EXECUTABLE_ACTION_IDS` set | region | COMMITTED |
| model-gateway artifacts.rs:137-232 (version store, lock_commit) | 137 `pub struct ArtifactVersionStore {` | region | COMMITTED |
| user-core space_capability_decision.go:100-175 (space:egress) | 158 `permissions = append(permissions, "space:egress")` | match (region) | COMMITTED |
| execution-core permission/mod.rs:259-272 (leaf refusal) | 259-260 comment and `execute_provider_action` | region | COMMITTED |
| proto events.proto:8-60 (envelope, cited by a:553) | 8 is an import line; envelope not re-read | UNVERIFIED | COMMITTED |

## 10. Result object (for the orchestrator)

- file: `.plan-work/haiku/cross-audit-overlap.md`
- overlap rows (section 2): 122
- cited paths classified: 73 (COMMITTED 58; MODIFIED-UNCOMMITTED 11; UNCOMMITTED-NEW 4; MISSING 0)
- top surprises: see sections 5.1 C1-C23 and 5.2 O1-O11; the largest are the client-supplied `space_ref` (C1), the queue ruling (C2), the grant scope vocabulary (C3), the origin vocabulary (C4), and the schedule owner (C9).

