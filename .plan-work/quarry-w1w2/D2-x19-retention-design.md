# X-19 condition (c): retention or erasure of stored step receipts (design only, nothing built)

Written 2026-10-28 by unit D2-B-x19-receipts. Status: a recommendation for the owner. It is not a decision. Nothing in this note is built, no migration exists, no flag exists.

## 1. The problem in one paragraph

Under the accepted default (`QUARRY_EDGE__RECEIPT_TEXT_MODE=sensitive`) a stored agent step receipt keeps ordinary typed text: a search query, a name typed into a form, an address typed into a field. Secrets are masked, but personal data that is not a secret is kept. Today nothing deletes a receipt: `quarry_step_receipts` is append-only by convention (no production code path updates or deletes a receipt row; only tests do), there is no TTL, and the Control Plane's erasure flow does not reach it. So ordinary personal data an agent typed stays in Postgres for as long as the database lives. X-19 says receipts must not go on for real users until there is a retention period or an erasure path.

## 2. What is stored, and where (checked in the code, 2026-10-28)

| Store | What it holds | Keys that identify the subject | Already reached by an erasure? |
|---|---|---|---|
| `quarry_step_receipts` (migrations 0007, 0008, 0010 to 0013, 0018) | `action` (typed text, key presses, URLs without secrets), `observation` (page text, title, URLs, every leaf passed through the Durable rules), `outcome` (failure text) | `org_id` (NOT NULL), `run_id`, `actor_id` (nullable: legacy rows have NULL), `receipt_id` | No |
| `quarry_agent_run_checkpoints` (0006) | `current_url` (masked), step counters | `org_id`, run id | No |
| `quarry_browser_timeline_events` (0009) | tab and action timeline items (class, ids, no URL or title by contract) | `org_id`, run id | No |
| `ActionStarted` events on the NATS stream (and the SSE stream) | the same masked action as the receipt, since this unit | run id, org id | Stream retention only (stream config, not row based) |
| Artifacts (screenshots, evidence deltas) | masked screenshots, JSON evidence | tenant stamp, artifact id | Artifact store lifecycle, separate question |
| `quarry_profiles` and tombstones, Redis profile keys | browser profiles | `org_id` | **Yes**, org erasure only (WS17.E3, `org_erasure.rs`, `DELETE /v1/internal/orgs/:org/profiles`) |

The last row is the precedent: Quarry already has an internal erasure surface and a trait for it. The receipts are the first family that holds free text typed by an agent, so they are the family that matters most.

## 3. Who owns erasure (architecture rules applied)

- **The Control Plane owns the decision and the trigger.** CLAUDE.md: identity, users, orgs, audit belong to the Control Plane. `user-core` (`gdpr_erasure_saga.go`) and `org-core` (`gdpr_erasure_fanout.go`) already publish `verevon.gdpr.erasure.requested` on the shared control bus with `subject_type` in `organization`, `user` and `user_anonymize` and the ids `subject_id`, `org_id`, `requested_by`, `ts` (`apps/Control Plane/CONTROL_PLANE_OWNERSHIP.md`, "Cross-plane erasure fan-out"). Consumers already exist in the Application Plane (conversation-core, org erasure) and the Data Plane (documents-api-go). Erasure is a Control Plane saga; the other planes purge their own copy.
- **The Ingestion Plane (Quarry) owns the deletion of its own rows.** No other plane may delete from `quarry_step_receipts` (no direct database crossing). The existing path is the Quarry-side `quarry-control` GDPR consumer (Go, `Quarry-v2/services/quarry-control/internal/gdpr/subscriber.go` and `erasure.go`; compose: `NATS_SHARED_USER: quarry-control-gdpr`). Today it acts only on `organization` events: it deletes the org's crawl data with `DELETE ... WHERE org_id = $1` and calls quarry-edge `DELETE /v1/internal/orgs/:org/profiles` with the shared internal token, and quarry-edge walks the `OrgErasable` families in `AppState`.
- **Retention (a time limit) is a Quarry decision**, because it is a property of a Quarry table. The product owner sets the number; Quarry runs the job.

So the answer to "which plane owns erasure": the Control Plane triggers it, Quarry (Ingestion Plane) executes it on its own tables, and nobody else touches those tables.

## 4. Options

| Option | What it is | Effort (rough) | Covers | Does not cover |
|---|---|---|---|---|
| A. Time-based retention only | A purge job deletes receipt rows older than N days (`finished_at`), and trims `observation` earlier than the rest if wanted | 2 to 3 days (job, flag, test, runbook) | Every subject, eventually; no Control Plane change | A person's right to erasure inside the window. Art. 17 needs "without undue delay", and N days of waiting is not that |
| B. Erasure on the Control Plane event only | Add a `StepReceiptErasure` family (a second `OrgErasable`), and extend the quarry-control consumer to act on `user` and `user_anonymize` events, deleting by `org_id` (org) or by `actor_id` (user) | 4 to 6 days (family, the actor-scoped delete, consumer change in Go, route rename, tests, runbook) | Org and user erasure requests, the legal minimum | Data of subjects that never ask; rows with NULL `actor_id`; a user's typed text about third parties |
| C. A and B together (recommended) | B for requests, A as the backstop so nothing lives forever | 6 to 9 days | Both | Same limits as below |
| D. Do not store ordinary typed text at all (`all` as the default) | Flip the default | 2 to 3 days | Removes the problem for new rows | Recorded procedures stop being replayable. The owner chose `sensitive` on 2026-10-28, so this is a decision to reopen, not an option I recommend |
| E. Per-org retention setting | A retention policy per org in the Control Plane | 8 to 12 days across planes | Flexible | New config surface nobody asked for |

## 5. Recommendation: C, in two steps, both forward only

**Step 1 (small, do first): erasure.**
1. A new `OrgErasable` family `step_receipts` in `quarry-runtime/src/org_erasure.rs`. `purge_org` runs `DELETE FROM quarry_step_receipts WHERE org_id = $1`, the same for `quarry_agent_run_checkpoints` and `quarry_browser_timeline_events`, and returns counts only (no id, no payload). It is idempotent by construction. It must delete in batches (for example 5,000 rows per statement) so one org with millions of rows does not hold a long lock.
2. A user-scoped purge: `DELETE ... WHERE org_id = $1 AND actor_id = $2`. `actor_id` is the signed initiating actor (migration 0008). Rows whose `actor_id` is NULL (written before 0008) cannot be attributed to a user and are reached only by org erasure and by retention. Say so in the runbook; do not guess a backfill.
3. The `user` and `user_anonymize` events carry `subject_id` but also `org_id`; the consumer needs the org to scope the delete, and a user can belong to several orgs. Open question for the Control Plane owner: does the event carry every org the user belonged to, or only the "primary" one? If only one, user erasure cannot be scoped correctly from the event alone and must run `WHERE actor_id = $1` across all orgs. That is acceptable for a hard delete (the actor id is globally unique) but it is a cross-tenant statement, so it must run only from the internal route with the internal token and must log counts only.
4. The quarry-control consumer (Go) is extended from "organization" to the user subject types, and the internal route is generalised: keep `DELETE /v1/internal/orgs/:org/profiles` as it is (profiles) and add `DELETE /v1/internal/orgs/:org/receipts` and `DELETE /v1/internal/actors/:actor/receipts`, or make the existing route walk all families. Prefer one route per subject type and the same family list, so the existing counts-only response shape holds.
5. ZDR: ZDR runs write no receipts, so nothing to erase for them.

**Step 2: retention backstop.**
1. A purge task in quarry-edge (or a Temporal schedule if the orchestrator owns schedules for Quarry; check before building): `DELETE FROM quarry_step_receipts WHERE finished_at < now() - $retention` in batches, plus the same for checkpoints and timeline events. Flag `QUARRY_EDGE__RECEIPT_RETENTION_DAYS`, default **unset = no deletion** (so it ships dark), and a refuse-to-boot rule in production when it is unset once receipts are switched on for real users (the same shape as the redaction mode refusals in `redaction_boot.rs`).
2. Proposed starting value for the owner to confirm: 30 days for `observation` and `action`, nothing finer. It is a product and counsel number; I do not set it. The Control Plane org purge runs after 30 days (`ORG_PURGE_DAYS`), so 30 matches the existing platform timer. Counsel decides whether an audit need (for example the receipts are evidence for a disputed agent action) justifies longer for failed or high-risk steps; if so, keep the `action` kind, locator and outcome class and null the `observation` and the typed text, instead of deleting the row.
3. Append-only is a convention, not a database rule, so nothing in the schema blocks the delete. If a trigger that forbids `UPDATE` and `DELETE` is ever added, the purge role must be exempt.

## 6. What is still not covered after C (state this to the customer plainly)

- `qr-redact/4` rows written before the D2-065 fix can hold a raw key press. The retention window bounds this; no one-off rewrite is planned. If counsel wants it gone sooner, the cheap action is a one-time `UPDATE ... SET action = ...` through a reviewed migration (receipts are otherwise never rewritten), or a `RULE_VERSION` bump with a read-time re-mask (needs the golden pin regenerated).
- Backups and replicas keep deleted rows until they age out. The Postgres backup retention is an infrastructure number; write it in the privacy text.
- The NATS event stream keeps `ActionStarted` events by stream retention, not by row. Set the stream `max_age` no longer than the receipt retention, in the Quarry stream config.
- Data a person typed about a third party is still the person's personal data and is erased with the person; nothing finds it by content.
- Typed text that is not a secret and was kept raw is, by the accepted default, personal data. The customer-facing wording (what Verevon says it keeps) is counsel's, not ours.

## 7. What the stack session must show before this is relied on

1. A receipt written by a real run, then `DELETE` through the internal route with the internal token: counts returned, zero rows left for that org (`psql`), a second call returns zeros.
2. An org erasure published on the shared bus (use the Control Plane dev stack): the Quarry consumer deletes the receipts, the profile families still go first, and the log carries counts only.
3. With retention set to a short value on a dev edge: old rows go, new rows stay, no query holds a lock longer than a second on a table of one million rows (measure, do not assume).

## 8. Effort and order

Step 1: 4 to 6 days (Quarry Rust plus one Go change in the quarry-control consumer, no Control Plane code if the event already carries the org). Step 2: 2 to 3 days. Total 6 to 9 days. Neither step needs the Control Plane to change its event, except the open question in 5.3.

## 9. Decisions this needs from the owner (not made here)

1. Retention number (days) and whether failed or high-risk steps keep a thinned row longer. Counsel to confirm for customer wording.
2. Is user-level erasure in scope for the first release, or org erasure only (org erasure alone is 2 to 3 days)?
3. The user-event org question in 5.3, to the Control Plane owner.
