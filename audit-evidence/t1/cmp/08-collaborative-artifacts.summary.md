# T1 / Category 8: Collaborative artifacts (cross-source comparison)

Inputs: 57 reference records (buzz, grokbot, qm, slack, teams) plus 9 Verevon baseline records. I grouped them into 26 clusters. I excluded 1 record: teams-product-surface-116, the live-event comment stream, which belongs to meetings. Coverage check: 56 assigned + 1 excluded = 57.

## Shared baseline (table stakes)

| Cluster | Sources | Verevon |
|---|---|---|
| C-001 Room canvas: a shared document per room that agents can read and write | buzz, grokbot, slack, teams (4/5) | **absent** |
| C-011 Room file library: uploads and attachments with an owner plane, provenance and quotas | teams, buzz, qm, slack (4/5) | **partial**: read-only Knowledge tab, no upload in the room |
| C-009 Artifact sharing: view/edit roles, general access, access requests | slack, qm, grokbot, teams (4/5) | **partial**: view-only document grants |
| C-004 Lists and boards in the room (common, 3/5) | slack, teams, buzz (issues only) | **partial**: per-thread todos only |

## Best of breed

| Capability | Best source | Why (mechanism to copy) |
|---|---|---|
| Room canvas | Slack (DOC) / Teams notes (REA); Buzz as the open model (SRC) | Rich blocks, comments, a conversation tab. Buzz saves each edit as an append-only revision, offers diff and restore, and lets agents use it through the CLI |
| Write safety | Buzz (SRC) + Grok Bot (SRC) | The server checks the expected head (CAS) and reads it from the writer, not a replica. Grok Bot adds lock/renew/release with fencing tokens and tombstone undelete with typed statuses |
| Typed room records | Buzz NIP-AR (SRC) | Stable id, typed records, operation enum, prev-CAS, idempotent resubmit. Edits never touch unread counts or notifications |
| Lists | Slack (DOC) | Typed fields, saved views, item threads that feed Activity, add a message to a list |
| Agent artifacts in the room | Grok Bot (SRC) | A packaged rule that analyses go into a canvas, plus tools to write and read canvases and documents. QM validates sandboxed HTML playgrounds before showing them |
| Publish agent apps | QM (SRC) | Immutable versions, rollback by flipping a pointer, an owner scope separate from the scope the app was created in, provider interface, scoped data replication |
| Sharing | QM (SRC) | Google-Docs-style dialog, request access, approve or decline in chat after an owner check |
| File library | QM (SRC) model, Teams (REA) product | Files addressed by SHA-256, with owner scope, created-in scope and creator. Teams: one storage authority for ACL and compliance |
| Files as knowledge | Slack (DOC) + Grok Bot (SRC) | Automatic summaries and retention. Grok Bot keeps the extracted text rather than the upload and returns typed refusal reasons |
| Inline viewers | Grok Bot (SRC) | Lazy-loaded viewers for PDF, DOCX, XLSX, code highlighting and Mermaid |
| Agent context from a page | Buzz (SRC, q5) | Untrusted values are quoted as data and capped, and the user sees the payload before sending |
| Agent task tracker | Grok Bot (SRC) | States Working, Needs input and Done, one thread per task, unread state per task |
| Live embeds in messages | Teams Loop (REA) | The message holds a pointer and the component keeps its own ACL, with a warning when sharing would widen access |
| Conversation snapshot share | QM (SRC) | Read-only copy limited to an allowlist of content, recheck of creator access, a page that needs no session |

## Differentiators (2+ sources, not most)

- Agent-produced artifacts in the room (grokbot, qm)
- Publishing agent-built apps (qm, grokbot)
- Write safety with CAS and locks (buzz, grokbot)
- Files as agent knowledge (grokbot, slack)
- Inline rich viewers (grokbot, slack)
- Provider link cards and cloud pickers (grokbot, slack)
- Annotating artifacts to give the agent feedback (qm, grokbot)
- Code collaboration in the room (buzz, qm). Out of scope for Verevon.

## Verevon's biggest gaps (priority tier)

1. **C-001 Room canvas (absent, significance 5, L).** Data Plane should own a `space_canvas` document with append-only revisions and a head CAS. Application holds only the pointer. Agents write through a Model tool under a delegated grant. Under ZDR, the canvas needs explicit policy. Today's Studio canvas lives in gateway memory and is lost on restart (`studio.rs:1-5`).
2. **C-006 Agent artifacts in the room (partial, significance 5, M).** `/chat` already ships an artifact panel with revisions, sandboxed HTML and DOCX/PPTX/XLSX export (`ChatArtifactPanel.tsx`, `chat-artifacts.ts`). The Space room reuses none of it. Lift that code into the room and persist the artifacts under the shared-read decision.
3. **C-011 Room file library and uploads (partial, significance 5, M).** The Knowledge tab is read-only and the composer has no attach control. Reuse the `/chat` documents upload and record Space provenance in Data Plane.

Recommended next: write safety (C-002), typed records (C-003), then lists (C-004), sharing extensions (C-009), files as knowledge (C-012), shared viewers (C-013), resource-bound case Spaces (C-020), untrusted-context framing (C-021) and the agent task tracker with Needs input (C-025).

Skipped as poor fits: desktop download manager, artifact windows, whiteboard, code hosting/PRs, docked terminal.

## Verevon strengths (keep)

- **Single grant authority.** `resource_grants` is enforced by retrieval and documents-api, and org and grantor are set on the server (`shares.rs`). Extend it rather than adding per-feature ACLs.
- **Durable Space workspace.** It already does an atomic CAS promote with conflict reporting (`sandbox-manager/internal/workspace/store.go:158`). Use it as the template for artifact heads.
- **Support Inbox draft lease (C-023, shipped).** Persisted drafts, a claim/release lease and AI reply proposals. This equals or beats QM's review queue. Generalize it to Space drafts.
- **Chat artifact contract** (same id with a higher version is an update, history kept) and the sandboxed-HTML viewer.

## Caveats

- Slack evidence is mostly DOC (public help pages). Teams evidence is REA of locale keys and chunks, so it shows that a feature exists, not how deep it goes. Knowledge crystallization (C-022) is DOC-only and was never built in Buzz.
- Prevalence counts for C-009, C-011 and C-013 include sources whose evidence sits in records clustered elsewhere (cross-referenced in each description).
- Several Verevon statuses are `unknown` and need a code check: C-015, C-016, C-021, C-022, C-024. C-013's DOCX/XLSX preview absence is an inference.
- Most shared Space features depend on per-org `space_effect_policies` rows. Those are deny-by-default and missing in dev, so even shipped pieces are largely unverified live.
