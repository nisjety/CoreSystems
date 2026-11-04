# W2-CP-X9EVENT status: X-9 event route, org-core ZDR-enabled event and quarry-control consumer (lane d, branch w23/lane-d)

## Built / partly
Partly. Both halves the scope names are built, with their tests. Three one-line wiring and provisioning rows sit outside the unit's files and are not done (see Not done). Until they are, the org-core emit is inert and the quarry-control consumer has no consumer row to bind.

What existed before:
- org-core already emits `controlplane.org.interactive_retention.enabled` (`{org_id, zdr: true}`, msg id `interactive-retention:<outbox id>`) for the explicit toggle. It goes through the interactive-retention outbox, only on a false-to-true change of the stored intent (`SetInteractiveRetention`). Conversation Core consumes it. This was not rebuilt.
- A plan change emitted nothing for ZDR. Effective ZDR is intent AND plan entitlement. The intent survives a downgrade, so the upgrade back turned ZDR on silently. That was the gap (Ask 2 in `X-9-control-plane-ask.md`).
- quarry-control had no consumer of the event.

org-core (`apps/Control Plane/org-core/internal/nats/`):
- `zdr_enabled_signal.go` [new]:
  - `SubjectInteractiveRetentionEnabled` and `RetentionPostureReader` (satisfied by `*orgcore.Service`).
  - `SharedPublisher.SetRetentionPostureReader` and `SharedPublisher.PublishZDREnabledForBillingSync`.
  - `emitZDREnabledOnPlanChange`. It acts only when the new plan is entitled and the previous one was not. It then reads the posture at publish time and publishes only if ZDR is in effect. A deleted org is skipped. A read or publish error is returned.
  - The message ids: `interactive-retention:plan:<org>:<revision>` and `interactive-retention:billing-sync:<org>:<plan>:<unixnano>`.
  - The payload is `{org_id, zdr: true, cause}`. Conversation Core's decoder ignores `cause`.
- `shared_publisher.go`: a new field `postureReader`. `PublishPlanChanged` publishes the plan event as before, then the ZDR signal. If the signal fails, the call returns the error, so the plan-change outbox row stays pending and is retried. Both message ids are stable per revision.
- `billing_sync_subscriber.go`:
  - The `orgService` field is now a small interface. The constructor signature is unchanged.
  - `SetZDRSignal` ignores a typed-nil `*SharedPublisher`.
  - With the signal on, the subscriber reads the posture before and after `ApplyBillingPlanSync` and emits on a false-to-true change. Nothing is emitted on an override pin or a failed sync. With the signal off, nothing changes and no posture is read.
- `zdr_enabled_signal_test.go` [new], the publisher unit tests:
  - the transition table (upgrade with intent publishes; without intent, entitled to entitled, downgrade and unentitled to unentitled do not, and the last three read no posture);
  - inert without a reader or an org id;
  - a deleted org is skipped, while a read error and a publish error are returned;
  - the message ids are stable and do not collide with the toggle's;
  - `zdrTurnedOn`;
  - the billing-sync emit happens only on a transition (not when already on, without intent, on an override or on a failed sync);
  - no posture read with the signal off, and a typed-nil publisher is ignored.
- `org/plan_change_test.go`: `TestPlanChangeCanTurnEffectiveZDROnWithoutAToggle`. It pins the premise: the intent is kept through a downgrade, and the upgrade makes ZDR effective again.

quarry-control (`apps/Ingestion Plane/Quarry-v2/services/quarry-control/`):
- `internal/posture/posture.go` [new]: `ModeFromEnv` (`QUARRY_CONTROL__ZDR_EVENT_CONSUMER`), `Consumer`, `Handle(data, deliveryKey)` and `MarkOrgZdr(org, cause, key)`.
  - It lists the event org's schedules with `ListByOrg` only, and change monitors are schedules with `target_kind=change_monitor`.
  - observe logs `would_pause=true` per enabled schedule and writes nothing.
  - enforce first writes a `schedule_skipped` event with `reason: skipped_zdr`, then pauses with `UpdateEnabledByOrg(id, org, false)`. The event carries `{schedule_id, target_kind, reason, cause?}`, no run, job, URL or org id. Its id is deterministic per (org, schedule, delivery key), so a conflict is "already written". A row deleted since the list (ErrNotFound) is skipped.
  - It is idempotent. A schedule that is already disabled is counted and left alone. A crash between the event and the pause is repaired on redelivery without a second event.
  - zdr absent or false, or no org_id, is a no-op. Malformed JSON is a poison error and is acked.
- `internal/posture/subscriber.go` [new]: `StartSubscriber` binds with `nats.Bind("CONTROLPLANE", "quarry-control-interactive-retention")` and `ManualAck`, filtered on `controlplane.org.interactive_retention.enabled`, the same pattern as gdpr. It refuses an off consumer. The ack/nak split is the gdpr one. The delivery key is the Nats-Msg-Id, else the stream sequence.
- `internal/posture/posture_test.go` [new], the Go consumer tests:
  - mode parsing (default off; a typo or "on" never enforces);
  - enforce pauses only the event org's enabled schedules and monitors, writes one reason event each, writes nothing for already-paused schedules or org-b, and leaves org-b's monitor enabled;
  - a redelivery is a no-op and writes no new events;
  - a crash between the reason event and the pause is repaired;
  - observe pauses and writes nothing;
  - off and the no-op payloads do nothing, and malformed JSON is poison;
  - StartSubscriber guards, the delivery key, and the reason event id (deterministic, org- and transition-scoped, `evt_` kind).
- `cmd/control/main.go`: after the GDPR consumer, it reads the flag. off (the default) logs and starts nothing. Otherwise it starts `posture.StartSubscriber` on the same control-shared connection (`gdprNC`, identity `quarry-control-gdpr`). With no connection it logs a warning and stays off.

## Commits (w23/lane-d)
- 35224852e feat(org-core): emit the ZDR-enabled event when a plan change turns ZDR on (X-9)
- 2b5d839c8 feat(quarry-control): ZDR-enabled event consumer in internal/posture (X-9)
- this status file and the lane-d depth rows (docs commit)

## Code only, not compiled
Code only. No cargo, pnpm or vitest run, and no `go test`. gofmt is clean on LF copies of every touched Go file. One deviation from the owner order: a `go vet ./` ran once by mistake in org-core `internal/nats` after the edits. It printed nothing, which means that package and its tests type-checked. Nothing else was compiled. quarry-control was not compiled at all.

## Flag and default
- `QUARRY_CONTROL__ZDR_EVENT_CONSUMER=off|observe|enforce`, default **off** (the subscriber is not started). `shadow` is read as observe. Any other value is off and logged.
- org-core has no flag. It closes a missing emit (a leak-class fix: consumers missed a ZDR-on transition). It is inert until main.go wires it, which is Not done row 1.

## Assumed decisions
- The flag name and default come from the queue row (`QUARRY_CONTROL__ZDR_EVENT_CONSUMER`, default off). The decision doc's `QUARRY_CONTROL__ZDR_RETENTION_EVENT` with default observe was superseded by the queue.
- The same subject and payload are used for the plan-change cause (the ask's first preference), plus a `cause` field. A separate subject was not used, so Conversation Core gets the transition with no change.
- The posture is read at publish time (outbox flush or after the billing sync), not inside the plan transaction. repository.go and service_enhanced.go are outside the unit's files. An intent cleared or a plan changed again before the flush is seen as it is now, so the event never claims ZDR that is not in effect.
- The billing path emits at most once, matching that subscriber's core-NATS tier. A failed emit is logged and not retried (see depth).
- "Pause" means `enabled=false`. That is the existing paused state, and the wire shows `paused: !enabled`. The schedule's `zdr` stamp is not rewritten, because no store method exists and store.go is outside the unit's files.
- Nothing is ever resumed automatically: there is no ZDR-off event. Prospective versus retroactive ZDR stays counsel's question before enforce. Pausing removes nothing already collected.
- The reason event has no run or job (`events.run_id` and `job_id` are nullable), so no API read surfaces it today (see depth).
- The consumer reuses the GDPR control-shared connection and identity rather than adding a second one.

## MP asks
None. No Model Plane change is needed.

## Not done (outside the unit's files; Control Plane rows the lead or integration owns)
1. org-core `cmd/server/main.go`: inside the `else if sp != nil` branch add `sp.SetRetentionPostureReader(orgService)`. After `NewBillingSyncSubscriber` add `billingSyncSubscriber.SetZDRSignal(sharedPub)`. That needs `sp` hoisted to a variable visible there, for example `var sharedPub *nats.SharedPublisher`. Without these lines the emit never fires.
2. audit-core `internal/provisioner/provisioner.go`: a consumer row next to `conversationInteractiveRetentionConsumerConfig`:
   - Durable and DeliverGroup `quarry-control-interactive-retention`;
   - DeliverSubject `_VEREVON.CONTROL.SHARED.DELIVER.ingestion.quarry-control.interactive-retention`;
   - FilterSubject `controlplane.org.interactive_retention.enabled`, AckExplicit, AckWait 30 s, MaxDeliver 20;
   - registered in the ensure list.
3. `apps/Control Plane/control-shared-nats.conf`, user `quarry-control-gdpr`, mirroring its org-erasure grants (lines 108-117):
   - publish `$JS.API.CONSUMER.INFO.CONTROLPLANE.quarry-control-interactive-retention`;
   - publish `$JS.ACK.CONTROLPLANE.quarry-control-interactive-retention.>`;
   - subscribe `_VEREVON.CONTROL.SHARED.DELIVER.ingestion.quarry-control.interactive-retention quarry-control-interactive-retention`.
4. Ingestion compose: `QUARRY_CONTROL__ZDR_EVENT_CONSUMER` is not added to the compose env, so it stays off by absence.
5. Docs: RUNBOOKS (resume a paused schedule; the D1-006 SQL remedy is now the fallback), the ZDR matrix texts and the X-9 ask note. They are not edited. Gap rows stay Gap.

## Not verified
- Nothing compiled or ran except the one accidental `go vet` on org-core `internal/nats`. The posture package, the quarry-control main.go and the org test are uncompiled.
- No live broker: the bind, the ack/nak path, the Nats-Msg-Id dedupe across the plan and ZDR messages, and Conversation Core receiving the new cause.
- No Postgres run of the reason event insert (no run_id, deterministic event_id conflict mapped to ErrConflict).
