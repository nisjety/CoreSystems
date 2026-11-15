# T1 / 01 Messaging core: cross-source comparison

Sources: buzz (29 records), GrokBot (18), QM (24), Slack (21), Teams (28). That is 120 records in 47 clusters, all assigned and none excluded. Verevon baseline: 39 code-verified records. Detail is in `01-messaging-core.clusters.json` (ids C-t1-01-001..047).

## Shared baseline (table stakes)
Most sources have these. Slack and Teams are often DOC/REA benchmarks.
- Human-first rooms where agents are ordinary members (001)
- DMs and group DMs (002)
- Threads that any member can reply in, with a panel and an option to broadcast to the room (003)
- Mentions bound to exact identities, plus group handles (006)
- Reactions (008)
- Edit and delete (011)
- Rich composer (013)
- Attachments (017)
- Voice and video clips (024)
- Drafts (026)
- Per-thread read state (035)
- Custom emoji (010)

## Best-of-breed
| Capability | Best source | Why |
|---|---|---|
| Human-first channel (001) | buzz | Agents are plain channel members. A post is a plain append, and an agent is invoked only when mentioned or subscribed (SRC). |
| Threads (003) | buzz (+QM semantics) | The server keeps thread_metadata counters and depth atomically at ingest. QM lets any scope member continue a thread, with a speaker label on each entry. |
| Mentions (006) | buzz | Recipients travel as structured ids, not body text. Namesakes get qualified labels and ambiguous names block the send (q5). |
| Edit/delete (011) | buzz | The owner of an agent can retract or edit that agent's post. Soft-deleted events stay in the audit log. |
| Edits seen by the agent (012) | QM | Edits and deletes become `message_revision` entries, appended outside a turn under a lease. |
| Attachments (017) | buzz / Slack | buzz stores files by content hash (sha256) and strips metadata. Slack uploads in 3 steps through an external URL, with a malware scan. |
| Attachments as model input (021) | QM | Access is re-checked every turn. The transcript stores references, not bytes. A route table picks native file input or a text fallback per provider. |
| Link previews (022) | buzz | The sender's side fetches the page with SSRF guards and signs a snapshot, so recipients never contact the third-party site. |
| Reactions to and from agents (009) | QM | Reactions are typed events that can wake the agent, capped at 5 per turn. A cheap helper model picks the acknowledgement emoji. |
| Send reliability (042) | GrokBot | A send journal, a client nonce and echo confirmation, with DUPLICATE and UNKNOWN_DURABILITY states (q5). |
| Conversation kind sets the scope (046) | QM | A DM maps to the person's personal scope, a channel or group to a shared scope. Each thread is its own session inside the scope. |
| Presence and status (037) | Teams | The richest availability model. A status message shows when someone @mentions you. |
| System messages (038) | Teams | 87+ typed notices, including agent added or removed. buzz signs notices on the relay so clients cannot forge them. |
| Read state (035) | buzz / Teams | buzz has a server-side read frontier plus manual unread. Teams marks read optimistically on the client and throttles the network calls. |

## Differentiators worth carrying
- Reactions as agent input and output (009)
- Typed content model with a fallback card for unknown types (016)
- Attachments re-authorized every turn (021)
- Fork that keeps visibility rules (033)
- Engagement rule: once an agent has a stake in a thread, it hears follow-ups without a new @mention (043)
- Conversation kind decides the scope (046)
- System messages in the timeline (038)
- Per-viewer translation (039)
- Owner-of-agent edit and delete (011)

## Verevon's biggest gaps (priority tier)
1. **No human-only post (001, sig 5).** Every room post calls `streamChat` and starts a model turn. Fix: Session Core gets an append that does not invoke the model, authorized by a Control `space:post` decision, plus the engagement rule (043). Without both, a Space is a shared AI prompt box.
2. **Non-owners cannot reply in a thread (003, sig 5).** Session Core `append_message` still calls `authorize_thread_owner`, but the UI offers Reply. There is also no thread panel and no reply counts.
3. **@person mentions are text only (006, sig 5).** There is no mention record and no notification; the notification-core allowlist blocks it. The `mentioned_agent_ref` pattern should be extended to people.
4. **No DMs or group DMs (002, sig 5).** Only `personal` and `room` kinds can be created.
5. **No attachments in rooms (017, sig 5).** The Chat documents path exists but Space does not use it. Planned owner: Data Plane documents with `space_ref`.
6. **No edit or delete (011), no reactions (008).** Both are absent from proto, gateway and Convex.

Recommended next:
- Rich composer, and human posts rendered with ChatMarkdown (013)
- Attachments as model input (021)
- Wire the queue and steer that already exist into the Space composer (045). Backend done, UI only.
- Per-thread read state (035)
- Pins by Space role, plus the ordering bug (028)
- Drafts (026)
- Link previews through Quarry (022)
- Deep links: SpacePage ignores `?thread_id` (023)
- Typed system messages (038)
- Content types (016)
- Idempotent send (042)
- Fork, already planned as S1.7 (033)
- Scope rules (046)

Skipped:
- Cross-post and move (032): conflicts with per-Space authority.
- Bearer share links (034): conflict with ZDR and the rule that Space access never widens an owner-plane ACL.
- Bridged external surfaces (047): Channel Plane is future work.
- OS recent-files shelf (019)
- Video timecode comments (025)

## Verevon strengths (keep)
- **Stop a turn (044).** A cancel that was recorded server-side is shown differently from one that only stopped locally.
- **Typing (036).** Content-free rows; each caller writes only its own row; 8 s decay.
- **Agent rendering (015).** Full ChatMarkdown for agent output.
- **Server-side @agent re-resolution.** The browser sends only a subject ref (006/007).
- **Author attribution from the verified caller only** (003).
- **Audience-revision-filtered room reads (041).** Already the right base for history windows and fork.
- **Room transcript reads gated by Control decisions.** The backend for queue and steer also exists.

## Caveats
- "Shipped" means present in source, not deployed. Release gates R-1..R-5 are still open.
- 8 clusters have Verevon status `unknown`: 004, 005, 014, 016, 038, 039, 040, 042. A later code check must verify them.
- Slack and Teams records are mostly DOC/REA, so their q-scores reflect product docs or locale strings, not code.
- 007 (buzz agent invite) and 034 (QM share) rest on DOC only. Polls (030) rest on one low-confidence REA record.
- `buzz-product-024` (scheduled send) and `buzz-product-025` (pins/saved/polls) are "not built" records. They are kept as members, but buzz is not counted in prevalence.
- For some clusters, prevalence counts sources evidenced by a record in a different cluster; each case is named in that cluster's bestInClass. Example: Slack and Teams DMs come from their channel-primitive records.
