# Model Plane runbooks

Operator procedures for the development stack and the release path. Each runbook says which stack it is for; production changes go through the signed artifact (`scripts/release-artifact.sh`), never a hand-rolled deploy.

| Runbook | What it is for |
|---|---|
| [audit-delivery-alerts.md](audit-delivery-alerts.md) | Audit delivery alerts: what each one means and what to do. |
| [audit-delivery-replay.md](audit-delivery-replay.md) | Audit delivery: deploy the publisher fix and replay the stuck rows. |
| [docker-desktop-recovery.md](docker-desktop-recovery.md) | Docker Desktop recovery on Windows, when the engine stops answering. |
| [migration-policy.md](migration-policy.md) | Session Core migrations: expand, then contract. |
| [model-entitlement-shadow-to-enforce.md](model-entitlement-shadow-to-enforce.md) | Model entitlement: shadow to enforce, and the live proof (WS10-S7). |
| [release-rollback-rehearsal.md](release-rollback-rehearsal.md) | Signed baseline, signed candidate and rollback rehearsal (WS01-S12). |
| [stable-prefix-order-ab-evidence.md](stable-prefix-order-ab-evidence.md) | Stable-prefix prompt order: staging A/B and evidence (WS02-S8). |
| [zdr-attestation-and-live-proof.md](zdr-attestation-and-live-proof.md) | ZDR: the ordered wave deploy, the fail-closed live proof (Z-60 pass 1), pass 2 after an attestation, and the attestation lapse alerts. Pass 1 was run on 2026-10-28: `docs/compliance/evidence/live-proof-wave1-zdr-2026-10-28.txt` at the repository root. |
