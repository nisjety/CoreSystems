# X-9: request to the Control Plane owners (Quarry and the ZDR-enabled event)

Date: 2026-10-28. From: the Quarry v2 work on branch `quarryv2-next` (local, not pushed). Decision: X-9 in `.plan-work/quarry-w1w2/DECISIONS-ROUND-2026-10-27.md`, section 3.7, accepted by the owner on 2026-10-28. Status of this note: a draft for the owner to send. Nothing on the Control Plane side has been changed by Quarry, and nothing in it is a request to change auth-core or token issuance.

## 1. What we ask for

Two small changes, both on paths that already exist.

**Ask 1. A consumer row for quarry-control on the ZDR-enabled event.**
- Subject: `aqencia.controlplane.org.interactive_retention.enabled`, published by org-core (`org-core/internal/nats/shared_publisher.go`, `PublishInteractiveRetentionEnabled`). Payload `{org_id, zdr: true}`, message id `interactive-retention:<outbox event id>`. Conversation Core already consumes it (`audit-core/internal/provisioner/provisioner.go`, `ConversationInteractiveRetentionConsumerName` and its delivery subject).
- We ask for: a durable consumer row for quarry-control in audit-core's provisioner, next to Conversation Core's (a name such as `quarry-control-interactive-retention` and a delivery subject under the same `_VEREVON.CONTROL.SHARED.DELIVER.` prefix you use), and the broker permissions for the quarry-control NATS user to receive on that delivery subject and to ack. Names are yours to choose; tell us what you pick.
- quarry-control already consumes a Control Plane event in the same way for GDPR erasure (`services/quarry-control/internal/gdpr/subscriber.go`), so the pattern, ack handling and redelivery behaviour are known on our side.

**Ask 2. Emit the same event when a plan change turns effective ZDR on.**
- Effective ZDR is the plan AND the stored intent (`org-core/internal/org/repository.go`, `GetRetentionPostureState` reads both). Today the outbox row, and therefore the event, is written only inside `SetInteractiveRetention`, when the intent goes from false to true (`repository.go`, the `zdr && !wasEnabled` branch). A plan change writes no outbox row.
- So an org whose stored intent is already true while its plan is not entitled (for example after a downgrade, or if a new org's intent defaults to true as the doc comment on `SetInteractiveRetention` suggests) gets effective ZDR silently when the plan becomes entitled again. Every consumer that relies on the event, Conversation Core included, misses that transition.
- We ask for: in the plan-change transaction, when effective ZDR goes from false to true (entitled now, intent true, not entitled before), insert the same outbox row so the same event goes out. Same subject, same payload. If you prefer a separate subject for the plan-change cause, that also works for us, as long as the payload carries `org_id` and the message id stays unique per transition.
- Please confirm or correct the reading above: (a) whether a new org's stored intent can be true while its plan is not entitled, and (b) whether a plan downgrade clears the intent or keeps it.

What we do NOT ask for: no `org:retention:read:any` scope for quarry-edge or quarry-control, and no fire-time lookup in org-core (the plan's old S4 route). Today only auth-core holds that scope, and the decision keeps it that way.

## 2. What Quarry will do with the event

All behind a new quarry-control flag, `QUARRY_CONTROL__ZDR_RETENTION_EVENT=off|observe|enforce`, default `observe`. Nothing is built yet; it is built after the stack-evidence session, in this order:

1. **Consumer** in quarry-control next to `gdpr/subscriber.go`: it receives `{org_id, zdr: true}` and calls an idempotent `MarkOrgZdr(org_id)`. A redelivered or duplicate event changes nothing the second time.
2. **Change monitors first** (the half that is live today). `MarkOrgZdr` stamps the org's monitors as ZDR. Under `observe` it only logs and counts what it would pause. Under `enforce` the monitors are paused and refused on the stamp, so they stop fetching and storing baselines. Today a monitor stamps the ZDR posture at creation and never re-checks it; the only remedy is operator SQL (`docs/RUNBOOKS.md`, around lines 539 to 570).
3. **The job-event tail** (D2-057): the edge refuses `follow_job` (live tail) under ZDR with the same code `errors_for` already uses, plus a test. This is on the edge, not on the event.
4. **Docs**: the ZDR matrix texts, its golden and the runbook are updated; gap rows stay marked Gap until enforce.
5. **Not reversed automatically**: an event for ZDR going true to false does not exist, and Quarry will not unpause anything on its own. Paused monitors stay paused; the runbook will say how an operator resumes them.

## 3. What stays waiting

- **Executable schedules** (scrape, crawl and batch schedules, WS15.S3 to S5): fully dark today (`QUARRY_SCHEDULED_JOBS_ENABLED=0`), sent with `zdr: None`, preflight answers allow/unchecked. Their half of the consumer is built only when scheduled jobs head for go-live. Scheduled crawls with no stored scope also bypass the robots gate (D2-318); that is fixed before schedules go live as well.
- **Enforce**: the owner decides after at least one week in `observe` on a live stack, together with ADR-4 stage B and the customer notice. Counsel answers the question in section 4 first.
- **The legacy job-event drain**: stays as the ZDR matrix documents it ("reads pre-ZDR jobs as before"). Changing it belongs to ADR-4 stage B.
- **D14 (pull model), D17 (notices), D9**: wait.
- **Optional extra on our side**: the edge can lazily re-stamp an org's schedules when it sees the ZDR floor true on a request. This is a safety net, not a replacement for Ask 2.

## 4. Counsel: prospective versus retroactive

To be answered before enforce, and not by engineering:

> When an org turns ZDR on after it already has change monitors (and, later, schedules), is ZDR **prospective** (stop collecting from now on) or **retroactive** (also remove what was collected before)?

What pausing a monitor does not remove today: baselines already stored for the monitored pages; the monitored URLs that sit in Temporal workflow arguments and history (D1-007); `change_detected` events already emitted. A retroactive answer needs a purge path for each of these, which is not designed. A prospective answer needs only the steps in section 2. The customer-facing wording of either answer is also counsel's.

## 5. Who to answer and how

Reply to the owner (the Quarry plan's decider). For Ask 1, the consumer and delivery names and the permission change. For Ask 2, the reading in the two questions above and where the plan-change emit would land (org-core PR or ticket). Quarry builds nothing that depends on Ask 2 until it is confirmed; the monitor half can start with Ask 1 alone, in `observe`.
