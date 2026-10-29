# leads-core native and multi-architecture build/deploy — 2026-07-13

The July 11 Rosetta/x86 TLS diagnosis is obsolete. On 2026-07-13 the host, Docker daemon, target image, and running `leads-core` container were arm64, and an authenticated tenant-shaped company-only search returned HTTP 200 from real Brreg. Do not rebuild merely to fix a failure that is no longer present.

This runbook makes that state reproducible and provides a targeted rollout if a later source change requires it.

## Read-only diagnosis

```bash
uname -m
docker version --format '{{.Server.Arch}}'
docker inspect --format '{{.Architecture}} {{.Os}} {{.Id}}' \
  "$(docker inspect --format '{{.Image}}' leads-core)"
docker inspect --format '{{.Image}} {{.Config.Image}}' leads-core
docker compose ps leads-core
curl --fail --silent --show-error http://127.0.0.1:3164/health >/dev/null
curl --fail --silent --show-error http://127.0.0.1:3164/ready >/dev/null
```

If architecture is native and the safe Brreg probe passes, investigate DNS, CA bundle, proxy, clock, and provider status before changing the image.

## Safe Brreg semantic probe

Use an approved internal key and a synthetic audit organization—not a customer. Query companies only and discard the response body:

```bash
test -n "$INTERNAL_API_KEY"
test "$(curl --silent --output /dev/null --write-out '%{http_code}' \
  --request POST http://127.0.0.1:3164/api/v1/leads/search \
  --header "X-Internal-Api-Key: $INTERNAL_API_KEY" \
  --header 'X-Org-Id: audit-native-build-20260713' \
  --header 'Content-Type: application/json' \
  --data '{"query":"Brønnøysundregistrene","limit":1}')" = 200
```

Do not call person/role endpoints, save a list, export CSV, or retain the response.

## Reproducible builds

Single native image from the Compose context:

```bash
docker compose build --pull leads-core
export LEADS_IMAGE_ID="$(docker compose images -q leads-core)"
docker image inspect --format '{{.Architecture}} {{.Os}} {{.Id}}' \
  "$LEADS_IMAGE_ID"
```

For CI multi-architecture publication, use an approved registry/repository and immutable revision; do not use `latest` as rollback identity:

```bash
export IMAGE_REPOSITORY='<approved-registry>/coresystem/leads-core'
export IMAGE_REVISION='<full-source-revision>'
docker buildx build \
  --platform linux/amd64,linux/arm64 \
  --file leads-core/Dockerfile \
  --tag "$IMAGE_REPOSITORY:$IMAGE_REVISION" \
  --provenance=mode=max \
  --sbom=true \
  --push \
  leads-core
docker buildx imagetools inspect "$IMAGE_REPOSITORY:$IMAGE_REVISION"
```

CI must run unit/race tests and image-internal DNS/CA/TLS checks per architecture. A manifest existing is not proof that the application can reach Brreg.

## Targeted rollout and rollback

Follow `application-plane-safe-deployment-2026-07-13.md`. Capture `leads-core`'s old image ID, build only `leads-core`, replace it with `docker compose up -d --no-deps leads-core`, then require health, readiness, no-auth/wrong-auth rejection, company-only Brreg HTTP 200, wrong-tenant negatives for stored lists, and honest upstream 502 behavior.

Rollback by retagging the recorded old image ID to the Compose image reference and targeting only `leads-core`. Do not recreate Postgres, NATS, or any unrelated service. Retain old/new digests and the redacted probe record through the rollback window.

## Provider classification

Brreg is a real production public-data provider, not a sandbox or mock. Local test transports/fixtures remain test-only. Record `fetched_at`, upstream error/degraded state, and cache provenance; never convert provider failure into fabricated success.
