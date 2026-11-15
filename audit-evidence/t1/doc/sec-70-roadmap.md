## 9. Roadmap and decisions

This section turns the 96 recommendations into an order of work. Tiers and efforts are the verified values: 23 priority, 42 recommended and 31 nice-to-have items after verification, with 29 effort estimates revised and 5 tier changes (F-t1-005 moved from priority to recommended; F-t1-029, F-t1-066, F-t1-068 and F-t1-069 moved from recommended to nice-to-have). Where verification changed an item's effort (25 up, 4 down), the table shows the revised value with the original in brackets. "Plane" lists the owning planes in the order the verification named them. "Depends on" lists only the sequencing dependencies that matter for planning; fuller lists are in the item cards (the Recommended cards carry a Dependencies and risks paragraph); Appendix B does not list dependencies.

### 9.1 Implementation waves

#### Wave 1: foundations and quick wins

Goal: people can post and reply without a model turn, on a substrate that pushes, with ZDR (Zero-Data-Retention) hardening, an admin surface for the deny-by-default policies, and cheap fixes that users notice at once.

| ID | Title | Tier | Effort | Plane | Depends on |
|---|---|---|---|---|---|
| [F-t1-001](#F-t1-001) | Human-first room: human-only posts, shared threads, thread pane | priority | L | Model Plane, Control Plane, Frontend Plane | ADR 1 (message ownership); the existing member-capable Control thread:append decision (no new decision type); [F-t1-007](#F-t1-007) so agents are summoned explicitly |
| [F-t1-003](#F-t1-003) | Realtime substrate: event projection, resumable SSE, delivery-time authorization | priority | XL | Model Plane, Application Plane, Frontend Plane, Control Plane | ADR 1 and ADR 2; a new Control stream audience; [F-t1-033](#F-t1-033) client store |
| [F-t1-005](#F-t1-005) | ZDR binding in every plane (residual hardening) | recommended (was priority) | S (was M) | Model Plane, Application Plane, Frontend Plane | [F-t1-008](#F-t1-008) for content-free notifications |
| [F-t1-006](#F-t1-006) | Space governance admin (effect-policy UI first) | priority | XL (was L) | Control Plane, Frontend Plane, Model Plane | Phase 1 stands alone; later phases need [F-t1-022](#F-t1-022), [F-t1-054](#F-t1-054), [F-t1-011](#F-t1-011) |
| [F-t1-014](#F-t1-014) | Agent artifacts and viewer registry in the room | priority | M | Frontend Plane, Model Plane | Shared-read policy rows in dev; [F-t1-015](#F-t1-015) for promote-to-canvas |
| [F-t1-022](#F-t1-022) | Room administration basics and org-room roster sync | priority | L (was M) | Control Plane, Application Plane, Frontend Plane | [F-t1-006](#F-t1-006) for orphaned rooms; deletion saga for restore window |
| [F-t1-024](#F-t1-024) | Client resilience: error boundaries, version gating | priority | M (was S) | Frontend Plane | [F-t1-032](#F-t1-032) drafts before any forced reload |
| [F-t1-030](#F-t1-030) | Room content hardening: silent 403, link safety, report-only CSP | recommended | M | Frontend Plane | None |
| [F-t1-031](#F-t1-031) | Org audit trail: role-gated read first | recommended | M | Control Plane | audit-core; [F-t1-006](#F-t1-006) admin role catalogue |
| [F-t1-033](#F-t1-033) | Reliable send: idempotency key, send states | recommended | M | Model Plane, Frontend Plane | Session Core dedupe (does not exist yet); [F-t1-003](#F-t1-003) for echo confirmation |
| [F-t1-063](#F-t1-063) | Agent avatar colour and hover card | recommended | S | Frontend Plane, Application Plane | Agent status projection (exists) |

The two large items here, [F-t1-001](#F-t1-001) and [F-t1-003](#F-t1-003), are the foundations almost everything else waits on, so they start first even though they finish late in the wave. Both need ADR 1 before code.

#### Wave 2: a real multi-human room

Goal: addressed agent wake-up, agent work visible to the whole room, mentions, notifications, a cross-room Home, DMs, attachments, edits and reactions, search, approvals that resume, SSO and SCIM, and the composer, timeline, links and command layer that make the room usable.

| ID | Title | Tier | Effort | Plane | Depends on |
|---|---|---|---|---|---|
| [F-t1-002](#F-t1-002) | Addressed agent wake-up and trigger policy | priority | L | Frontend Plane, Application Plane, Model Plane, Control Plane | [F-t1-001](#F-t1-001); Control turn_origin (ADR 1); [F-t1-004](#F-t1-004) for reason cards |
| [F-t1-004](#F-t1-004) | Agent work visible to the whole room | priority | XL | Model Plane, Frontend Plane | [F-t1-003](#F-t1-003); [F-t1-001](#F-t1-001); [F-t1-012](#F-t1-012) labels; a Control room-audience run-read decision |
| [F-t1-007](#F-t1-007) | Identity-bound @mentions | priority | M | Frontend Plane, Model Plane, Application Plane | [F-t1-001](#F-t1-001); an owner-plane notification producer; [F-t1-032](#F-t1-032) draft restore |
| [F-t1-008](#F-t1-008) | Space notifications with levels and follow or mute | priority | XL (was L) | Application Plane, Model Plane, Control Plane | Mention records; notification-core producer allowlist; a badge-semantics decision; ADR 2 for ZDR orgs |
| [F-t1-009](#F-t1-009) | Cross-room attention: aggregate stream and Home inbox | priority | L | Application Plane, Model Plane, Frontend Plane | [F-t1-008](#F-t1-008); [F-t1-003](#F-t1-003); [F-t1-016](#F-t1-016) for a DMs destination |
| [F-t1-010](#F-t1-010) | Approvals with scopes and durable resume | priority | L | Model Plane, Control Plane, Frontend Plane | [F-t1-004](#F-t1-004) card surface; Control grant records |
| [F-t1-013](#F-t1-013) | Room attachments and file library | priority | L | Data Plane, Frontend Plane, Control Plane | A Control shared-import validator; [F-t1-057](#F-t1-057) phase 1; [F-t1-046](#F-t1-046) for agent use |
| [F-t1-016](#F-t1-016) | DMs and group DMs | priority | L | Control Plane, Application Plane, Model Plane | [F-t1-001](#F-t1-001); a Control DM kind (ADR 1); [F-t1-052](#F-t1-052) |
| [F-t1-017](#F-t1-017) | Edit and delete as revisions, reactions | priority | L | Model Plane, Application Plane, Control Plane, Frontend Plane | [F-t1-001](#F-t1-001); Control policy flags and audit event |
| [F-t1-018](#F-t1-018) | Permission-checked message search | priority | L | Model Plane, Frontend Plane, Control Plane | ADR 9; reuse of the room-read decision; [F-t1-001](#F-t1-001) if replies are searchable |
| [F-t1-021](#F-t1-021) | Agent computer confinement: resource caps | priority | M (was L) | Model Plane, Ingestion Plane, Control Plane | sandbox-manager leases; [F-t1-006](#F-t1-006) policy fields; ADR 6 for egress |
| [F-t1-023](#F-t1-023) | Enterprise identity: SCIM fix, SSO, deprovisioning | priority | XL (was L) | Control Plane | [F-t1-043](#F-t1-043) cascade; [F-t1-054](#F-t1-054) groups; [F-t1-048](#F-t1-048) durable membership events |
| [F-t1-032](#F-t1-032) | Composer for real work | recommended | L | Frontend Plane | [F-t1-001](#F-t1-001); [F-t1-007](#F-t1-007); [F-t1-013](#F-t1-013); [F-t1-015](#F-t1-015) |
| [F-t1-034](#F-t1-034) | Scalable timeline: server windows, virtualised rows | recommended | L (was M) | Model Plane, Frontend Plane | [F-t1-003](#F-t1-003) seq; [F-t1-033](#F-t1-033); [F-t1-004](#F-t1-004) row model |
| [F-t1-039](#F-t1-039) | Permalinks and entity router | recommended | L | Frontend Plane, Model Plane, Application Plane | [F-t1-001](#F-t1-001); [F-t1-017](#F-t1-017) for quote tombstones |
| [F-t1-040](#F-t1-040) | Per-thread run queue and member-safe steering | recommended | L | Model Plane | [F-t1-001](#F-t1-001); [F-t1-003](#F-t1-003); [F-t1-032](#F-t1-032) |
| [F-t1-041](#F-t1-041) | Command layer: palette, switcher, slash commands | recommended | M | Frontend Plane | Action registry (shipped); [F-t1-055](#F-t1-055); [F-t1-009](#F-t1-009) |
| [F-t1-042](#F-t1-042) | Read state, presence and reminders | recommended | L | Application Plane, Model Plane | [F-t1-001](#F-t1-001) for a per-thread frontier; [F-t1-009](#F-t1-009) |
| [F-t1-048](#F-t1-048) | Room directory and self-join | recommended | L | Control Plane, Model Plane, Frontend Plane | [F-t1-022](#F-t1-022); the membership-writer decision (9.4); durable membership events |
| [F-t1-049](#F-t1-049) | People and agent directory | recommended | M | Control Plane, Application Plane, Frontend Plane | [F-t1-016](#F-t1-016); [F-t1-053](#F-t1-053) for external lookup |
| [F-t1-052](#F-t1-052) | Typed entries, system messages, link previews | recommended | L | Model Plane, Application Plane, Ingestion Plane, Frontend Plane | [F-t1-001](#F-t1-001); a Control system-append class; [F-t1-015](#F-t1-015) and [F-t1-058](#F-t1-058) for embeds |
| [F-t1-055](#F-t1-055) | Sidebar organisation | recommended | M | Application Plane, Frontend Plane | [F-t1-009](#F-t1-009) for attention sections; [F-t1-048](#F-t1-048) |
| [F-t1-057](#F-t1-057) | Artifact sharing: edit role, Space-as-subject grants | recommended | L (was M) | Control Plane | The disabled grant write surface; [F-t1-015](#F-t1-015) and [F-t1-013](#F-t1-013) resource types |
| [F-t1-062](#F-t1-062) | UI quality baseline | recommended | M | Frontend Plane, Control Plane | Control user profile; [F-t1-008](#F-t1-008) preferences |

#### Wave 3: agents as colleagues

Goal: personal agents with audience-safe memory, a room-scoped agent browser, an agent self-API, the room canvas, cards and forms, the tool cockpit, automations and webhooks, cost and budgets, push outside the tab, moderation, retention and runtime guardrails.

| ID | Title | Tier | Effort | Plane | Depends on |
|---|---|---|---|---|---|
| [F-t1-011](#F-t1-011) | Personal agents, access list, private assistant | priority | XL (was L) | Control Plane, Application Plane, Model Plane, Frontend Plane | [F-t1-012](#F-t1-012); [F-t1-002](#F-t1-002); ADR 3 and ADR 4 |
| [F-t1-012](#F-t1-012) | Audience-safe agent context and memory | priority | L | Model Plane, Data Plane, Control Plane | ADR 3 (memory ownership); [F-t1-031](#F-t1-031) audit |
| [F-t1-015](#F-t1-015) | Room canvas | priority | L | Data Plane, Control Plane, Frontend Plane, Application Plane, Model Plane | A Control shared-write decision; [F-t1-057](#F-t1-057) phase 1 |
| [F-t1-019](#F-t1-019) | Agent self-API on spaces.* actions | priority | L (was M) | Model Plane, Control Plane, Frontend Plane | model-eligibility admission bar; Control run-bound decisions; [F-t1-010](#F-t1-010) resume; ADR 5 |
| [F-t1-020](#F-t1-020) | Room-scoped agent browser with live view and takeover | priority | L | Ingestion Plane, Model Plane, Control Plane, Frontend Plane | [F-t1-004](#F-t1-004); [F-t1-012](#F-t1-012); [F-t1-010](#F-t1-010); [F-t1-001](#F-t1-001) AuxPane; ADR 6 |
| [F-t1-026](#F-t1-026) | Typed cards and agent input requests | recommended | L | Frontend Plane, Model Plane | Run signal API; [F-t1-010](#F-t1-010); S3.4 broker for secret fields |
| [F-t1-027](#F-t1-027) | Agent operations: cost, budgets, allow/block, evals | recommended | XL (was L) | Model Plane | Before [F-t1-025](#F-t1-025); Data v2 citations |
| [F-t1-035](#F-t1-035) | PWA and content-free push | recommended | XL (was L) | Frontend Plane, Application Plane | [F-t1-008](#F-t1-008) producers; [F-t1-060](#F-t1-060) ledger; [F-t1-024](#F-t1-024); [F-t1-039](#F-t1-039); ADR 2 |
| [F-t1-036](#F-t1-036) | Room automations | recommended | XL (was L) | Control Plane, Model Plane | Shared-audience Control schedule contracts; [F-t1-019](#F-t1-019); [F-t1-010](#F-t1-010); [F-t1-027](#F-t1-027) caps |
| [F-t1-037](#F-t1-037) | Tool cockpit and reliable connections | recommended | L | Model Plane | S3.4 broker; S3.6 cockpit; [F-t1-026](#F-t1-026) sign-in card |
| [F-t1-038](#F-t1-038) | Agent history search and cited answers | recommended | L | Model Plane, Control Plane | [F-t1-018](#F-t1-018) index; ADR 5 and ADR 9 |
| [F-t1-043](#F-t1-043) | Moderation | recommended | XL (was L) | Application Plane, Control Plane, Model Plane | [F-t1-022](#F-t1-022); [F-t1-011](#F-t1-011) owner link; [F-t1-031](#F-t1-031); ADR 4 |
| [F-t1-044](#F-t1-044) | Credential custody and outward actions | recommended | XL | Ingestion Plane | [F-t1-028](#F-t1-028) egress; [F-t1-010](#F-t1-010) grants |
| [F-t1-045](#F-t1-045) | Runtime guardrails for shared rooms | recommended | M (was L) | Model Plane | [F-t1-005](#F-t1-005) typed refusals; S2.7 provenance envelope |
| [F-t1-046](#F-t1-046) | Files as agent input and knowledge | recommended | L | Model Plane, Data Plane | [F-t1-013](#F-t1-013); [F-t1-015](#F-t1-015) for pinning |
| [F-t1-047](#F-t1-047) | Webhooks that wake agents | recommended | L (was M) | Ingestion Plane, Model Plane | A Control shared-room fire decision; [F-t1-010](#F-t1-010) |
| [F-t1-051](#F-t1-051) | Retention and shared-Space deletion | recommended | XL (was L) | Control Plane, Application Plane, Model Plane | Saga migrations in staging; [F-t1-031](#F-t1-031); [F-t1-006](#F-t1-006); ADR 8 |
| [F-t1-056](#F-t1-056) | Agent task tracker | recommended | L (was M) | Model Plane, Application Plane | [F-t1-009](#F-t1-009); [F-t1-001](#F-t1-001) shared reply; S4.5 cockpit |
| [F-t1-059](#F-t1-059) | DND and quiet hours | recommended | M | Application Plane | [F-t1-008](#F-t1-008); [F-t1-060](#F-t1-060) for the flush |
| [F-t1-060](#F-t1-060) | Delivery ledger and digest | recommended | L | Application Plane | [F-t1-008](#F-t1-008); [F-t1-009](#F-t1-009); provider ZDR proof |
| [F-t1-061](#F-t1-061) | Catch me up | recommended | M | Model Plane, Frontend Plane | [F-t1-009](#F-t1-009) read markers (shipped) |

#### Wave 4: cooperation at scale and enterprise depth

Goal: multi-agent rooms, persistent per-agent computers, calls and meeting intelligence, guests and teams, lists, templates, governed skills, external meetings and voice.

| ID | Title | Tier | Effort | Plane | Depends on |
|---|---|---|---|---|---|
| [F-t1-025](#F-t1-025) | Multi-agent rooms | recommended | XL | Model Plane | [F-t1-002](#F-t1-002); [F-t1-004](#F-t1-004); [F-t1-027](#F-t1-027); [F-t1-010](#F-t1-010); ADR 5 |
| [F-t1-028](#F-t1-028) | Persistent per-agent computer | recommended | XL | Model Plane | [F-t1-027](#F-t1-027); [F-t1-044](#F-t1-044); [F-t1-012](#F-t1-012); ADR 6 |
| [F-t1-029](#F-t1-029) | Room calls on a bought SFU | nice-to-have (was recommended) | L | Application Plane, Frontend Plane, Control Plane | ADR 7; [F-t1-003](#F-t1-003); [F-t1-001](#F-t1-001); [F-t1-008](#F-t1-008) |
| [F-t1-050](#F-t1-050) | Meeting intelligence with consent | recommended | XL (was L) | Model Plane, Data Plane, Control Plane | A Control shared-room import decision; ASR provider; [F-t1-029](#F-t1-029) record; ADR 7 |
| [F-t1-053](#F-t1-053) | Guests and link invites | recommended | XL (was L) | Control Plane | [F-t1-022](#F-t1-022); [F-t1-048](#F-t1-048); billing seats; ADR 10 |
| [F-t1-054](#F-t1-054) | Teams and sub-groups | recommended | L | Control Plane | [F-t1-022](#F-t1-022); [F-t1-048](#F-t1-048); membership-writer decision |
| [F-t1-058](#F-t1-058) | Typed records and Lists | recommended | L | Application Plane, Frontend Plane | [F-t1-057](#F-t1-057) phase 1; [F-t1-009](#F-t1-009); ADR-0001 planner migration |
| [F-t1-064](#F-t1-064) | Room templates and case Spaces | recommended | L | Application Plane, Control Plane | [F-t1-011](#F-t1-011); [F-t1-022](#F-t1-022); [F-t1-015](#F-t1-015); [F-t1-058](#F-t1-058) |
| [F-t1-065](#F-t1-065) | Governed skills and plugin bundles | recommended | XL (was L) | Model Plane, Control Plane, Ingestion Plane | S3.5 skill-store choice; [F-t1-037](#F-t1-037) |
| [F-t1-066](#F-t1-066) | Agents attend external meetings | nice-to-have (was recommended) | L | Ingestion Plane | [F-t1-050](#F-t1-050); ADR 7 (capture ownership) |
| [F-t1-067](#F-t1-067) | Duplex voice for agents | recommended | L | Model Plane | Realtime route (exists); [F-t1-029](#F-t1-029) for phase B; [F-t1-050](#F-t1-050) consent |
| [F-t1-068](#F-t1-068) | Screen sharing | nice-to-have (was recommended) | XL (was M) | Frontend Plane | [F-t1-029](#F-t1-029) |
| [F-t1-069](#F-t1-069) | Search operators | nice-to-have (was recommended) | L | Frontend Plane | [F-t1-018](#F-t1-018) |

#### Wave 5: horizon

Goal: nice-to-have polish and speculative bets, built on demand once the earlier waves hold.

| ID | Title | Tier | Effort | Plane | Depends on |
|---|---|---|---|---|---|
| [F-t1-070](#F-t1-070) | Header and pulse rail extras | nice-to-have | M | Frontend Plane, Model Plane | [F-t1-027](#F-t1-027) for the meter; [F-t1-037](#F-t1-037) |
| [F-t1-071](#F-t1-071) | Agent templates and catalogue | nice-to-have | M | Application Plane, Control Plane | [F-t1-065](#F-t1-065) |
| [F-t1-072](#F-t1-072) | Team layer and promotion path | nice-to-have | XL | Model Plane, Control Plane | [F-t1-054](#F-t1-054); [F-t1-011](#F-t1-011); ADR 3 |
| [F-t1-073](#F-t1-073) | Tauri desktop shell | nice-to-have | XL | Frontend Plane, Control Plane | [F-t1-035](#F-t1-035); [F-t1-039](#F-t1-039); [F-t1-024](#F-t1-024) |
| [F-t1-074](#F-t1-074) | Provenance footer | nice-to-have | S (was M) | Model Plane, Application Plane | [F-t1-008](#F-t1-008) for the notification part |
| [F-t1-075](#F-t1-075) | Agent external channels | nice-to-have | XL | Ingestion Plane, Application Plane, Control Plane, Channel Plane | Channel Plane; S5.1 identity links; ADR 10 |
| [F-t1-076](#F-t1-076) | Public API | nice-to-have | L | Frontend Plane, Control Plane, Application Plane | Control token issuance; action registry |
| [F-t1-077](#F-t1-077) | Compliance export and holds | nice-to-have | XL | Control Plane, Data Plane | [F-t1-051](#F-t1-051); [F-t1-031](#F-t1-031); ADR 8 |
| [F-t1-078](#F-t1-078) | Latency observability | nice-to-have | M | Frontend Plane | [F-t1-003](#F-t1-003) |
| [F-t1-079](#F-t1-079) | Report flow and error registry | nice-to-have | M | Frontend Plane, Application Plane | [F-t1-024](#F-t1-024) |
| [F-t1-080](#F-t1-080) | ZDR-safe client persistence | nice-to-have | M | Frontend Plane, Application Plane | [F-t1-033](#F-t1-033); [F-t1-009](#F-t1-009); ADR 2 |
| [F-t1-081](#F-t1-081) | Agent long tail | nice-to-have | L (was M) | Frontend Plane, Application Plane | [F-t1-044](#F-t1-044); [F-t1-012](#F-t1-012) |
| [F-t1-082](#F-t1-082) | Messaging long tail | nice-to-have | L (was M) | Frontend Plane, Model Plane | [F-t1-013](#F-t1-013); [F-t1-052](#F-t1-052) |
| [F-t1-083](#F-t1-083) | Federated search | nice-to-have | L | Frontend Plane, Ingestion Plane | [F-t1-018](#F-t1-018); [F-t1-069](#F-t1-069) |
| [F-t1-084](#F-t1-084) | Meeting recording | nice-to-have | L | Data Plane | [F-t1-029](#F-t1-029); [F-t1-050](#F-t1-050) |
| [F-t1-085](#F-t1-085) | Live meeting AI | nice-to-have | L (was M) | Application Plane, Model Plane | [F-t1-029](#F-t1-029); [F-t1-050](#F-t1-050); [F-t1-067](#F-t1-067) |
| [F-t1-086](#F-t1-086) | Call polish | nice-to-have | M | Frontend Plane | [F-t1-029](#F-t1-029) for the call-only slices |
| [F-t1-087](#F-t1-087) | Snapshot share | nice-to-have | L (was M) | Application Plane | A Control snapshot decision class; [F-t1-013](#F-t1-013) |
| [F-t1-088](#F-t1-088) | App publish lifecycle | nice-to-have | XL | Application Plane | [F-t1-014](#F-t1-014); [F-t1-057](#F-t1-057); isolated hosting origin |
| [F-t1-089](#F-t1-089) | Multi-window | nice-to-have | M | Frontend Plane, Application Plane | [F-t1-039](#F-t1-039); [F-t1-042](#F-t1-042); [F-t1-055](#F-t1-055) |
| [F-t1-090](#F-t1-090) | Shell conveniences | nice-to-have | M | Frontend Plane | [F-t1-073](#F-t1-073) |
| [F-t1-091](#F-t1-091) | Realtime ops backlog | nice-to-have | M | Frontend Plane, Application Plane | [F-t1-003](#F-t1-003); [F-t1-006](#F-t1-006) flags |
| [F-t1-092](#F-t1-092) | Delivery health | nice-to-have | M (was S) | Application Plane, Frontend Plane | [F-t1-060](#F-t1-060); [F-t1-035](#F-t1-035); [F-t1-008](#F-t1-008) |
| [F-t1-093](#F-t1-093) | Multi-org | nice-to-have | XL | Control Plane | [F-t1-053](#F-t1-053); [F-t1-023](#F-t1-023); Channel Plane; ADR 10 |
| [F-t1-094](#F-t1-094) | Assurance long tail | nice-to-have | L | Control Plane, Ingestion Plane | [F-t1-031](#F-t1-031); a stable Space authority ADR |
| [F-t1-095](#F-t1-095) | Long-tail UX | nice-to-have | L | Frontend Plane | [F-t1-001](#F-t1-001) AuxPane; [F-t1-003](#F-t1-003); [F-t1-008](#F-t1-008) |
| [F-t1-096](#F-t1-096) | Reverse channel to the user's machine | nice-to-have | XL | Endpoint Plane | ADR 6; [F-t1-004](#F-t1-004) live viewing |

### 9.2 Quick wins

Eleven items keep the quick-win flag and a verified effort of S or M. Each one is cheap because the backend half already exists and the remaining work is wiring, a one-line fix or a Frontend-only change.

- [F-t1-005](#F-t1-005) ZDR hardening (S). The enforcement matrix, typed refusal catalogue and cross-plane refusals are already built; what is left is committing work in progress, typing two refusal sites and one cross-check. The original premise that Session Core ignores ZDR was wrong. [SRC]
- [F-t1-007](#F-t1-007) Identity-bound mentions (M). @agent is already re-resolved server-side; people mentions need mention records and an owner-plane notification consumer, not a new transport. It grows to L only if @room and role-gated groups are in the same slice. [SRC]
- [F-t1-014](#F-t1-014) Artifacts in the room (M). Session Core already persists artifacts; the Model Plane step drops out and the work is a Frontend panel plus an aggregate read. [SRC]
- [F-t1-024](#F-t1-024) Client resilience (M). Boundaries, chunk recovery and a build-id banner are S; the contract-version header and draft-safe reload make the whole item M. [SRC]
- [F-t1-030](#F-t1-030) Room content hardening (M). The deny-by-default server gate and the sandboxed agent-HTML path exist; the restricted-state fix for the swallowed 403 at apps/Frontend Plane/verevonv3/src/features/spaces/components/SpaceRoomTimeline.tsx:201 and link rules are S, and only the report-only CSP rollout is M. [SRC]
- [F-t1-031](#F-t1-031) Org audit trail (M). The role-gating fix for the audit read path is S and should be tracked as its own W1 security item; the admin role is already carried in the signed delegation claim but unused. [SRC]
- [F-t1-033](#F-t1-033) Reliable send (M). Generating the idempotency key in the composer is small; the Session Core dedupe is the main cost and the store refactor is optional. [SRC]
- [F-t1-041](#F-t1-041) Command layer (M). Phase 1 is Frontend-only over the shipped typed action registry and existing Control-gated reads, so it cannot widen what a viewer sees. [SRC]
- [F-t1-055](#F-t1-055) Sidebar organisation (M). The organiser pattern already exists for chat and preferences are per-user presentation that touch no Space ACL. [SRC]
- [F-t1-059](#F-t1-059) DND and quiet hours (M). A stored but unenforced per-user quiet-hours setting and a profile availability status already exist in Control; the build extends them. [SRC]
- [F-t1-061](#F-t1-061) Catch me up (M). The read marker it needs is shipped; a stateless, gateway-composed recap over the viewer's own per-thread read decisions is the buildable path. [SRC]

Six items kept the flag in the item data but verification raised their effort to L, so only a slice of each remains cheap: [F-t1-022](#F-t1-022) (the roster reconcile and rename or topic are S each; role change and archive touch Control's contract), [F-t1-034](#F-t1-034) (the client-only virtualisation slice is S), [F-t1-019](#F-t1-019), [F-t1-047](#F-t1-047), [F-t1-056](#F-t1-056) and [F-t1-057](#F-t1-057) (verification says these are not quick wins). [F-t1-063](#F-t1-063) is S without the flag: the stored avatar colour is never shown because of a one-line gateway omission. [SRC]

Near-free slices inside larger items that belong in W1: read the `?thread_id` parameter that Activity already emits ([F-t1-001](#F-t1-001), [F-t1-039](#F-t1-039)); schedule `syncAllOrganizationRooms` ([F-t1-022](#F-t1-022)); make the SCIM stub return a 501 instead of fake success ([F-t1-023](#F-t1-023)); wire the existing queue and steer backend into the Space composer ([F-t1-032](#F-t1-032), [F-t1-040](#F-t1-040)). [SRC]

### 9.3 Decisions needed before building (ADR candidates)

Ten decisions conflict with, or stretch, plane ownership, ZDR or Space authority. The numbering below is used in the wave tables.

**ADR 1. Ownership of human room messages and shared-thread append authority.** Affects [F-t1-001](#F-t1-001), [F-t1-002](#F-t1-002), [F-t1-003](#F-t1-003), [F-t1-016](#F-t1-016), [F-t1-017](#F-t1-017). Question: which plane is the source of truth for human posts, and who may append to a shared thread? Today Session Core in the Model Plane owns threads and its append calls an owner-only check at apps/Model Plane/rust/services/session-core/src/grpc.rs:4504, while the event projection lives in the Application Plane. [SRC] Options: keep Session Core as the single transcript owner, reuse the existing member-capable Control thread:append decision and add a turn_origin field (direct, ambient, automation) and DM kinds; or move human posts to an Application store, which creates two write paths on one transcript. Recommendation: keep Session Core as the owner. Verification found that AppendMessage already carries the Space decision fields and that Session Core already refuses durable writes for ZDR callers, so the change is a decision-subject fix plus a no-invoke route, not a new store. [SRC]

**ADR 2. Realtime delivery tier under ZDR and Space authority.** Affects [F-t1-003](#F-t1-003), [F-t1-004](#F-t1-004), [F-t1-009](#F-t1-009), [F-t1-035](#F-t1-035), [F-t1-080](#F-t1-080). Question: what may travel on a push wire, and how is authority re-checked on a connection that outlives a two-minute Control decision? Options: frames carry content and authorization is checked at subscribe only (fast, leaks on revocation); or frames carry ids and kinds only, authority is checked at subscribe and at every delivery, push is a content-free wake and ZDR orgs get no content cache. Recommendation: the second option, which is also what buzz does at its single fan-out chokepoint. [SRC] Verification adds that the current Control audiences are closed to Session Core and sandbox-manager, so a stream needs a new Control audience rather than a reused read token, and that notification-core persists no payload and never projects to the in-app feed for ZDR orgs, so the ADR must also say what in-app signals ZDR orgs get. [SRC]

**ADR 3. Agent memory ownership, audience labels and declassification.** Affects [F-t1-012](#F-t1-012), [F-t1-011](#F-t1-011), [F-t1-072](#F-t1-072). Question: who owns agent memory, and which plane enforces the audience fold? The adoption plan's S3.1 splits ownership between Data v2 (authored documents) and Model Plane (learned memory and resolver), and verification found that a Space message is already turned into the thread owner's user-level memory with no Space tag, so the leak vector exists today. [SRC] Options: label at write and fold at read in the Model Plane with Letta receiving the same filter; or exclude personal memory from shared rooms until labels exist. Recommendation: ship the containment slice (no auto-copy of room facts into personal memory) first, then the labelled model; keep personal agents out of shared rooms until it lands.

**ADR 4. Personal agents as service subjects.** Affects [F-t1-011](#F-t1-011), [F-t1-043](#F-t1-043), [F-t1-023](#F-t1-023). Question: how does Control admit an owner's agent to a shared room, and what cascades when the owner is revoked, banned or deprovisioned? Control refuses to replace a personal Space's membership, trusts Application's declared service roster, and holds no agent-to-owner link. [SRC] Options: owner-derived admission with the link recorded as a Control fact; or an Application-only add policy, which verification says would be advisory. Recommendation: the Control fact, since every cascade (ban, SCIM suspension) depends on it.

**ADR 5. Multi-agent delegation and whose authority an agent acts under.** Affects [F-t1-025](#F-t1-025), [F-t1-019](#F-t1-019), [F-t1-038](#F-t1-038). Question: when an agent delegates, or several humans trigger one run, whose decision applies? Today a room agent is a persona overlay on one human's turn and the UI-2c plan is scoped to one hop in the same room. [SRC] Options: a delegate re-resolves its own authority through Control, and the narrowest authority applies when several humans trigger a run; or children inherit the parent's decision. Recommendation: re-resolve per hop and record the initiating human on every hop. Verification notes that each Space run is owner-bound today, so the multi-human rule is mostly moot for v1 of history search. [SRC]

**ADR 6. Agent computer network boundary, persistence and local execution.** Affects [F-t1-021](#F-t1-021), [F-t1-028](#F-t1-028), [F-t1-020](#F-t1-020), [F-t1-096](#F-t1-096). Question: does agent egress go through Quarry with an audience-intersected allowlist, how does a durable per-agent home coexist with ZDR, and can a user's own machine ever execute tool requests? Verification corrected the baseline: network is already off by default for every Space code path, the lease is already required for Space code_interpreter (not for every Space agent), and Quarry already owns the SSRF boundary; the open gaps are resource caps, a Quarry egress route and the absence of ZDR handling in sandbox-manager. [SRC] Options: keep "Model proposes, Quarry executes" and define an equivalent policy point for any new executor class; or let sandboxes reach the network directly. Recommendation: the first; a local executor needs a registered plane and a Control device-identity contract before any code.

**ADR 7. Calls and meetings.** Affects [F-t1-029](#F-t1-029), [F-t1-050](#F-t1-050), [F-t1-066](#F-t1-066), [F-t1-067](#F-t1-067). Question: does Verevon reverse the huddle non-goal in apps/Frontend Plane/verevonv3/docs/space-defenition.md, buy an SFU, and model a meeting as a sub-resource of a Space or as a child Space kind? [SRC] Options: a bought SFU with Control-authorized join and an Application call record keyed by spaceRef; a child Space kind, which would load the Control registration saga and amend ADR-0001; or keep the non-goal and build the upload-based meeting MVP only. Recommendation if calls are wanted: sub-resource, bought SFU, and the shared-room Control import decision first, because verification found that decision, not ASR, is the critical path. [SRC] Capture ownership (Channel Plane versus Ingestion) must be decided in the same ADR.

**ADR 8. Retention, legal hold and ZDR precedence.** Affects [F-t1-051](#F-t1-051), [F-t1-077](#F-t1-077), [F-t1-005](#F-t1-005). Question: in what order do a legal hold, a retention sweep and a ZDR refusal apply? ZDR is all-or-nothing today, no plane has a retention sweeper, and legal hold exists only inside the personal deletion saga. [SRC] Options: hold over retention with ZDR refusals unchanged, enforced by every owner plane's own sweeper with honest receipts; or a central sweeper, which would need cross-plane DB access. Recommendation: the first; Session Core's TTL sweeper is the first phase.

**ADR 9. Message search index placement and exclusion rules.** Affects [F-t1-018](#F-t1-018), [F-t1-038](#F-t1-038). Question: where does the index live and what does it exclude? Options: in Session Core, excluding ZDR and privacy-restricted turns at index time, re-authorizing every hit and purged by deletion and retention; or a copy in Data v2 or Convex, which creates a second durable store of Space content. Recommendation: Session Core, with the existing model.thread.read decision reused rather than a new decision type. [SRC]

**ADR 10. External surfaces and guests.** Affects [F-t1-075](#F-t1-075), [F-t1-053](#F-t1-053), [F-t1-093](#F-t1-093), [F-t1-023](#F-t1-023). Question: where do guest identities live, and how do external channels reach a room while the Channel Plane is docs-only? Options: guests as a distinct subject type in auth-core with the S5.1 external-identity link graph; or guests as plain org members, which would expose org-visible documents and the organization room. Recommendation: the distinct subject type, with QM's rule that an external audience restricts agents to a context without org memory or keys. [SRC] Verification notes that the adopted S5.1 ADR file lives outside the repo and was not read. [INF]

### 9.4 Dependency notes

1. **Human posting before everything that attaches to a post.** [F-t1-001](#F-t1-001) unlocks [F-t1-007](#F-t1-007), [F-t1-016](#F-t1-016), [F-t1-017](#F-t1-017), [F-t1-039](#F-t1-039), [F-t1-040](#F-t1-040), [F-t1-042](#F-t1-042) and [F-t1-052](#F-t1-052). [F-t1-016](#F-t1-016) stays L only if [F-t1-001](#F-t1-001) carries the Session Core non-owner append work; otherwise it becomes XL.
2. **The push substrate before anything live.** [F-t1-003](#F-t1-003) is the consumer that makes [F-t1-004](#F-t1-004), [F-t1-009](#F-t1-009), [F-t1-034](#F-t1-034), [F-t1-035](#F-t1-035), [F-t1-040](#F-t1-040) and [F-t1-029](#F-t1-029) possible. Until it lands, the Space page polls every 6 seconds and sidebar dots stay browser-local. [SRC]
3. **Notification producers before push and digests.** [F-t1-008](#F-t1-008) must give notification-core Space, run and approval producers before [F-t1-035](#F-t1-035), [F-t1-059](#F-t1-059), [F-t1-060](#F-t1-060) and [F-t1-092](#F-t1-092) can deliver anything; the gateway may only notify the signed-in user today. [SRC]
4. **Decide who writes room membership before directory, guests and teams.** Application's declarative full replace into Control deactivates any managed user not declared, so a Control-native self-join, guest or team-derived membership would be revoked at the next sync unless the sync is scoped by source. This gates [F-t1-048](#F-t1-048), [F-t1-053](#F-t1-053) and [F-t1-054](#F-t1-054). [SRC]
5. **Approval resume before anything that pauses a run.** [F-t1-010](#F-t1-010) precedes [F-t1-019](#F-t1-019), [F-t1-026](#F-t1-026), [F-t1-036](#F-t1-036) and [F-t1-047](#F-t1-047). The continuation dispatcher exists for three action kinds and is refused under ZDR, so ZDR rooms cannot resume by design. [SRC]
6. **Memory containment before personal agents enter shared rooms.** [F-t1-012](#F-t1-012) precedes [F-t1-011](#F-t1-011), [F-t1-020](#F-t1-020) and [F-t1-028](#F-t1-028).
7. **Cost caps and confinement before multi-agent and persistent computers.** [F-t1-027](#F-t1-027) and [F-t1-021](#F-t1-021) precede [F-t1-025](#F-t1-025) and [F-t1-028](#F-t1-028); Session Core reports 0 tokens today and only an org-level hard stop exists. [SRC]
8. **A Control shared-room decision class before any durable shared write.** Import, schedule, watch and document-write decisions exist only for personal Spaces, which blocks [F-t1-013](#F-t1-013), [F-t1-015](#F-t1-015), [F-t1-036](#F-t1-036), [F-t1-047](#F-t1-047) and [F-t1-050](#F-t1-050) until Control gains shared-audience contracts. [SRC]
9. **Grant write surface before canvas, files and Lists.** [F-t1-057](#F-t1-057) phase 1 (the disabled resource_grants write surface) must land before [F-t1-015](#F-t1-015), [F-t1-013](#F-t1-013) and [F-t1-058](#F-t1-058) can ship without widening the uploader's ACL. [SRC]
10. **Policy rows before anything shared is verified live.** [F-t1-006](#F-t1-006) phase 1 is needed because dev has no space_effect_policies rows, so shared features are largely unverified live. [SRC]

## 10. Risks and open questions

### 10.1 Main risks

1. **Push tier bypass (architecture, ZDR).** A realtime tier that caches decisions beyond their two-minute TTL, fans out content, or skips the per-delivery check would widen access and break ZDR. Everything rides on it. NATS on the shared bus is plain and best-effort, so revocation latency can reach the decision TTL unless the audience revision is re-checked on each heartbeat. [SRC] Items: [F-t1-003](#F-t1-003), [F-t1-004](#F-t1-004), [F-t1-009](#F-t1-009), [F-t1-035](#F-t1-035).
2. **Index and copy sprawl (data governance).** Search indexes, notification payloads, previews, digests, caches and the BFF's Dragonfly copy of chat content are all second copies that need ZDR exclusion and deletion and retention hooks. No plane has a retention sweeper today. [SRC] Items: [F-t1-018](#F-t1-018), [F-t1-008](#F-t1-008), [F-t1-052](#F-t1-052), [F-t1-060](#F-t1-060), [F-t1-051](#F-t1-051).
3. **Cross-room leakage through agent memory (authority).** Verification confirmed that a Space message already becomes the thread owner's user-level memory with no Space tag, and that Letta would be the bypass unless it gets the same filter. [SRC] Items: [F-t1-012](#F-t1-012), [F-t1-011](#F-t1-011), [F-t1-025](#F-t1-025).
4. **Audit read path (security).** The suspected defect is confirmed by trace: the audit read path lets any org member read all org events, and the admin role carried in the signed delegation claim is unused. This is a W1 bug fix, not a feature. [SRC] Items: [F-t1-031](#F-t1-031).
5. **Governance that looks done but is not (security, delivery).** The SCIM stub returns fake success from the wrong plane, SSO is flag-only, and deny-by-default policies have no admin tooling. Each looks finished to a buyer. [SRC] Items: [F-t1-023](#F-t1-023), [F-t1-006](#F-t1-006), [F-t1-005](#F-t1-005).
6. **Runaway cost and loops (delivery, cost).** Multi-agent rooms, ambient participation, unattended triggers and broadcast mentions to agent groups multiply paid turns; cost-core has no space or agent dimension and the ZDR path refuses request-level spend caps. [SRC] Items: [F-t1-027](#F-t1-027), [F-t1-025](#F-t1-025), [F-t1-002](#F-t1-002), [F-t1-036](#F-t1-036), [F-t1-054](#F-t1-054).
7. **ZDR orgs get a degraded room (data governance, product).** Session Core refuses durable Space writes for ZDR callers, wiki-store refuses every mutation, attachments and imports are refused, continuation descriptors are refused and notification-core projects nothing to the in-app feed. Without a product decision, ZDR orgs will have no human posts, canvas, files, resume or in-app signals, and the UI must say so rather than fail silently. [SRC] Items: [F-t1-005](#F-t1-005), [F-t1-001](#F-t1-001), [F-t1-015](#F-t1-015), [F-t1-013](#F-t1-013), [F-t1-008](#F-t1-008), [F-t1-010](#F-t1-010).
8. **Plane-ownership drift (architecture).** Several items are tempting to build in the gateway or in Convex for speed: human posts, reactions, tombstones, the canvas, meeting records, agent definition revisions and the people directory. Verification corrected the owning plane for at least nine items. [SRC] Items: [F-t1-003](#F-t1-003), [F-t1-008](#F-t1-008), [F-t1-023](#F-t1-023), [F-t1-043](#F-t1-043), [F-t1-049](#F-t1-049), [F-t1-054](#F-t1-054), [F-t1-074](#F-t1-074), [F-t1-077](#F-t1-077), [F-t1-088](#F-t1-088).
9. **Membership-writer clobber (architecture, security).** The declarative replace from Application into Control can silently revoke self-joins, guests and team-derived members, and a lost plain-NATS revocation event can leave a deprovisioned user in rooms. [SRC] Items: [F-t1-048](#F-t1-048), [F-t1-053](#F-t1-053), [F-t1-054](#F-t1-054), [F-t1-023](#F-t1-023), [F-t1-043](#F-t1-043).
10. **Stale docs and a conflicting product definition (delivery).** Several docs contradict the code, and space-defenition.md lists huddles, chat parity and notification polish as non-goals while the brief asks for a Teams, Slack and Grok Bot hybrid. Plans must be re-checked against code and the stance recorded. [SRC] Items: [F-t1-029](#F-t1-029), [F-t1-050](#F-t1-050), [F-t1-068](#F-t1-068), [F-t1-035](#F-t1-035), [F-t1-059](#F-t1-059).

### 10.2 Open questions

1. Does Verevon accept the hybrid brief over the "no chat parity" and "huddles are a non-goal" lines in space-defenition.md? This decides [F-t1-029](#F-t1-029), [F-t1-050](#F-t1-050), [F-t1-068](#F-t1-068) and the notification items.
2. Which plane owns human room messages and reactions, and is there one source of truth for a thread that both people and agents write to? (ADR 1)
3. Approval resume is answered by verification: the execution-core worker resumes three idempotent action kinds and fails closed for shell, browser and MCP tools, while the model-gateway decide path still returns a deliberate quarantine placeholder and the room card says every tool continues. What remains open is how the card should state that difference before [F-t1-010](#F-t1-010) and [F-t1-026](#F-t1-026) are scoped. [SRC]
4. Who writes room membership: Application's declarative replace, a source-scoped sync, or Control directly? ([F-t1-048](#F-t1-048), [F-t1-053](#F-t1-053), [F-t1-054](#F-t1-054))
5. What do ZDR orgs get in push, in-app signals, previews, search, digests, canvas and files? Is a refs-only, no-resume experience acceptable to them, and should Space ZDR stay a flag that "changes nothing"? ([F-t1-005](#F-t1-005), [F-t1-008](#F-t1-008))
6. Who owns agent memory (S3.1), must Letta-backed memory be audience-labelled, or is it excluded from shared rooms until then? ([F-t1-012](#F-t1-012))
7. Should the always-on default room agent remain once human-only posting exists, or become an explicit binding with mention-only wake? ([F-t1-002](#F-t1-002))
8. Whose authority applies when one agent turn is triggered by several humans, or when an agent delegates to another? (ADR 5)
9. Are rooms private by default with org-public as an opt-in per org, as the UI research intent says, or Slack-style public by default? What is the late-joiner history rule? ([F-t1-048](#F-t1-048))
10. What does deleting or hiding a DM mean, given that deletion authorization covers personal Spaces only? ([F-t1-016](#F-t1-016))
11. Are the deletion saga migrations 022, 023, 0026 and 0027 applied in staging? The item states they are not; verification did not check. ([F-t1-051](#F-t1-051))
12. Does the Data Plane honour the room recipient audience for org-visible documents when a guest is present? Not traced. ([F-t1-053](#F-t1-053))
13. Calls: buy or self-host the SFU, and is a meeting a sub-resource of a Space or a child Space kind? (ADR 7)
14. Is the Channel Plane scheduled? External channels, linked identities, cross-org rooms, chat-app push and live meeting capture all wait on it. ([F-t1-075](#F-t1-075), [F-t1-093](#F-t1-093), [F-t1-066](#F-t1-066))
15. Is a Tauri desktop shell wanted after the PWA, or is web plus PWA enough? ([F-t1-073](#F-t1-073))

### 10.3 Evidence limits that matter for decisions

- Slack evidence is mostly public documentation plus the desktop shell; threads, canvas, retention, approvals, upload scanning and the streaming contract are DOC-only, so Slack shows that a feature exists, not how it is built. [DOC]
- Teams evidence is mostly localized strings and feature flags read from shipped files that REA parsed only partially (see Appendix A.3), plus Learn documentation; the inbox slices, thread pane, HITL consent, meeting AI governance and policy precedence are names and copy, not traced behaviour, and several are likely license-gated or in preview. [REA] [DOC]
- Grok Bot evidence is shipped-bundle source of medium confidence for the items that rest on it alone: the meeting bot, voice front, sandbox enforcement (the enforcing binary is not shipped, so only the client contract is visible), teach mode and standing grants (implemented only for messaging subjects in this build). [SRC] [INF]
- Several reference "best practices" are documented intent rather than code: buzz's low-noise notification defaults and knowledge crystallization contradict or are absent from its own client, buzz's information-flow control is a draft with an unused crate, and QM's ambient-turn rule is specific to its Slack surface. [DOC] [SRC]
- Artifacts-in-the-room ship on by default in 0 of 5 references (Grok Bot's canvas is flag-gated, QM's playground is unregistered), so [F-t1-014](#F-t1-014) rests on Verevon's own need rather than on proven reference behaviour. [SRC]
- Reference-side verification covered 24 items (22 partial, 2 confirmed); the other 72 items carry the comparison data as written, and 17 prevalence counts were corrected. On the Verevon side, 68 of 96 items are partial and nine verdicts were demotions, so item text that still reads "absent" should be read against the verified status in Appendix D. [INF]
