# Docker Content-Store Recovery Runbook

Status: **restart attempted 2026-07-12; Docker Desktop stopped after the VM disk failed to become ready**. The pre-restart inventory showed 99 containers across the six canonical Compose projects, 82 named volumes, and 10 networks. No prune, reset, volume deletion, or container recreation was performed.

Pre-restart evidence captured 2026-07-12: Docker client/server 29.5.3; canonical
projects Application (16), Control (18), Data v2 (15), Frontend v3 (3),
Ingestion (18), and Model (21). Several long-running services already report
unhealthy before maintenance, including Control identity services, the Velion
gateway, and Data infrastructure; those states must not be attributed to the
restart itself.

Recovery result: both the app-level restart and `docker desktop restart`
stalled at `still waiting for disk to be ready`; the API socket never answered.
The Docker data image is `/Volumes/Applikasjon/DockerDesktop/DockerDesktop/Docker.raw`
(278 GiB allocated). Its APFS host volume is 99% full with about 3 GiB free.
`diskutil verifyVolume /Volumes/Applikasjon` completed successfully, so the host
APFS structure appears sound. Docker Desktop was stopped to avoid an endless
start loop. Freeing/moving host data or resetting Docker is a separate,
destructive operator decision and was not attempted.

## Preconditions and evidence

1. Announce a maintenance window and freeze unrelated deploys.
2. Record `docker version`, `docker info`, `docker ps -a`, compose projects, image IDs/digests, networks, and volume names. Do not print environment values or secrets.
3. Render and securely retain the exact production Compose configuration with secret values redacted.
4. Normally, take and verify application-consistent backups of Postgres, Qdrant, MinIO/CAS, Temporal persistence, and NATS/JetStream. For this maintenance event only, the operator explicitly waived a new backup; record all readable volume/image/config evidence and preserve every existing volume in place.
5. Record current host-curl liveness and the known content-store errors. Confirm rollback image tags/digests exist outside the damaged local content store or in the registry.

## Approved maintenance action

1. Stop accepting traffic at the gateway/load balancer.
2. Gracefully stop the affected Compose projects where Docker still responds. Do not remove volumes.
3. Restart Docker Desktop/containerd using the operator-approved platform procedure. Do not prune, factory-reset, delete container data, or recreate volumes.
4. Verify daemon health, storage driver, volumes, networks, and registry access before starting workloads.

## Recovery order

1. Postgres, Dragonfly/Redis, NATS/JetStream.
2. Temporal, Qdrant, MinIO, SearXNG.
3. Control Plane/Auth Core, then Data Plane, then Model Plane.
4. Ingestion migrations and APIs: Quarry Control/Edge/orchestrator, Integration, Imports, Finspo, Shipping, Autocomplete. Keep support automation disabled.
5. Application Plane notification dependencies, Frontend Gateway, then UI.

Run migrations once per owning service. Build only tested revisions and attach OCI revision/build-time labels.

## Promotion gates

- Every required readiness endpoint is green and reports effective dependencies.
- Auth negative tests reject missing, malformed, wrong-audience, conflicting-org, and cross-org requests.
- Execute only read-safe/synthetic tests described in `INGESTION_PLANE_ROADMAP.md`.
- Do not book freight, schedule pickups, publish provider writes, import private production data, or persist live scrapes.

## Rollback

1. Remove traffic from newly deployed application containers.
2. Stop only the new application revisions; do not delete volumes.
3. Restore the recorded prior image digests and configuration.
4. If a migration is incompatible, use its reviewed down migration or restore the verified database backup; never improvise destructive SQL during the incident.
5. Re-run readiness/auth/data-integrity gates before restoring traffic.

Escalate and stop if volumes are missing, database integrity checks fail, registry images are unavailable, or the content-store I/O error persists after the approved restart.
