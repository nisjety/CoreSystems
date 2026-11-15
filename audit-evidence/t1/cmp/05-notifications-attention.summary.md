# T1 / Category 5: Notifications & attention (cross-source comparison)

Inputs: 79 reference records (buzz 17, grokbot 7, qm 6, slack 31, teams 19) plus 17 Verevon baseline records. Output: 26 clusters, all 79 records assigned, 0 excluded. Clusters file: `05-notifications-attention.clusters.json`.

## Shared baseline (what the table stakes are)
- **Event-to-notification pipeline** (C-003; slack, teams, grokbot, buzz). Events are sorted into a few signal classes and turned into one payload that every channel shares. The payload deep-links to the exact room, thread or message.
- **Personal cross-room activity inbox** (C-001; teams, slack, buzz). Mentions, replies, reactions, needs-action items and reminders all land in one list, with filters.
- **Unread counts across all rooms in the sidebar, and mark-all-read** (C-013; teams, slack, buzz).
- **Notification level per conversation** (all / mentions / off), with per-event switches and a hide-preview option (C-004; slack, teams, buzz).
- **Reminders and snooze** (C-019), **sounds per event** (C-018) and **OS badge or tray cues** (C-014). These are common, but they matter less.

## Best of breed
| Capability | Best source | Why |
|---|---|---|
| Inbox slices (C-001) | Teams (`teams-product-surface-060`) | About 60 typed activity kinds, plus slices for Mentions, Threads, VIP, Reminders, Drafts and Requests, each with show rules. Buzz-054 is the open, re-implementable version and has a Needs-action lane. |
| Agent "needs you" queue (C-002) | Grok Bot (`-076`, `-114`) | One queue holds approvals, questions, hand-offs and failed routines, with a reason shown. QM-034 has the best transport: one SSE stream of state changes that rejects stale updates and supports resync. |
| Notification pipeline (C-003) | Slack (`slack-shell-and-native-064`) | One validated payload passes DND and display gates and carries a trace id and click/reply telemetry. Grok Bot-074 is the best contract for agent turns ("finished" vs "awaiting you"). |
| Preferences (C-004) | Slack (`-049`, `-074`) | A global level with per-conversation exceptions, separate desktop and mobile settings, and a no-text-in-notifications flag. Buzz-055 shows the rule order: mention, then mute, then thread rules. |
| Unread aggregates (C-013) | Teams (`teams-shell-and-native-065`) | Counters across all accounts, checked for consistency. Slack-054 defines which events may raise a badge. |
| Badge projection (C-014) | Grok Bot (`-157`) | The badge is computed from the same data as the sidebar, skips muted or hidden items, and never moves backwards (epoch and sequence checks). |
| OS toasts and inline reply (C-012) | Slack (`-065`, `-066`) | Grouped toasts with a reply box and action buttons. The reply goes straight to the right thread, and logs keep no content. |
| Mobile push (C-026) | Buzz (`-058`, `-031`) | The push is a content-free "wake" signal. The client then fetches the message itself, so neither the push provider nor the gateway sees content. |
| Reminders (C-019) | Buzz (`-057`, `-106`) | The reminder is encrypted to its author, and the server only sees when it is due. |
| Delivery outbox (C-022) | QM (`qm-product-036`) | Uses idempotency keys, leased claims, acks, expiry and retries, and reports errors back to the run that sent the message. |
| Self-diagnosis (C-017) | Slack (`-056`, `-070`) | Only Slack has it: typed reasons why notifications are not arriving. |

## Differentiators
- **Agent "needs you" queue** (Grok Bot, QM).
- **Content-free wake push** (Buzz; it fits ZDR).
- **Agent as the channel for announcements** (Grok Bot-077).
- **OS focus awareness** (Slack, Teams desktop).
- **Delivery-method failover and health diagnosis** (Slack only).
- **Provenance footer** saying "Sent for @x" or naming the schedule (QM only).
- **Low-noise defaults** (Buzz). These are documented intent only: Buzz's own client does not enforce them.

## Verevon's biggest gaps
Ranked by tier, then significance:
1. **No Space event produces a notification** (C-003, absent, priority, 5). The notification-core allowlist only covers `run_completed` and `run_failed`. `mention.received` is seeded but nothing produces it, and @mentions are plain text.
2. **No personal cross-room inbox** (C-001, partial, priority, 5). Activity is scoped to one room.
3. **No unread counts across rooms** (C-013, partial, priority, 5). Durable `lastReadAt` exists, but the sidebar dots come from a store in the browser that only the open room fills.
4. **No follow or mute for threads and rooms** (C-005, absent, priority, 4).
5. **Recommended next:**
   - DND, schedules and urgent breakthrough, planned in S4.4 (C-007).
   - Notification level per room (C-004).
   - Web push as a content-free wake (C-026). Today the delivery worker refuses ZDR async delivery and Novu is off.
   - Service-worker notifications with Open and Reply (C-012). Approvals must not be one-click from a toast, because they are signed Control decisions.
   - "Catch me up" summaries (C-021).
   - Reminders (C-019).
   - Rate limiting (C-010).
   - Low-noise defaults, which are a decision to make before launch (C-008).
   - Provenance footer (C-023).
   - A content-free digest (C-020).
   - Finishing the S4.4 ledger (C-022).
6. **Skip:**
   - Keyword alerts: S4.3 agent watches are a superset.
   - OS focus awareness: there is no desktop shell.
   - A separate social agent feed: only its burst-collapsing rule is worth borrowing for Activity.

## Verevon strengths (keep these)
- **Attention inside a room is at least as good as the references.** It has the Pulse rail Attention count, a Needs-attention lens with salience ordering (critical > attention > normal > muted), and working indicators on the post, the composer and the sidebar. Approvals are signed, effect-bound Control decisions, which no reference has (C-002).
- **The delivery-attempt model is stricter than QM's.** `sent_unconfirmed` and `unknown` states exist, ZDR attempts are content-free, and async ZDR delivery is refused (`notification-core/internal/notification/delivery_state.go:13-22`, `delivery_worker.go:91-94`).
- **Shipped and reusable:** per-user run watchers ("notify me when this run finishes"), the notification-core preference storage, and durable read markers.

## Caveats
- **Slack and Buzz records are partly DOC-only, so treat them as intent.** For Slack this covers DND, cross-device routing, keyword alerts and catch-up. For Buzz it covers the zero-notification defaults and the desktop-suppression spec, which are marked as drafts. Teams product-surface records come from locale and string keys (REA), so their behaviour is inferred from UI strings.
- **Prevalence counts may understate how common a capability is.** They are based on cluster members, so capabilities that real Slack has but this record set does not show (thread follow, mute, mobile push) count as fewer sources. C-018 and C-019 also count sources whose evidence sits in another cluster; this is noted in each `bestInClass.why`.
- **Seven clusters have status `unknown` and need a code check:** C-006, C-015, C-016, C-017, C-018, C-019 and C-023.
  - I ran light read-only greps. They found no Space-level keyword alerts, sounds, toast system, reminders or attribution fields. Snooze exists only for support tickets.
  - C-021 is set to `partial` on the strength of the Meeting Scribe template (`SpaceCreateAgentDialog.tsx:43-54`), which comes from a baseline record outside this category.
- **Native-shell clusters (C-009, C-014 native part, C-016) assume Verevon stays web-first.** Revisit them if a desktop client is planned.
