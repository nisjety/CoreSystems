# Gateway result cache: tenant incident record and clean-up (WS4.B)

## What the defect was

Before WS4.A the gateway cached `POST /api/v1/search/web` and the scrape preview under
`verevon:gw:{namespace}:{hash}`. The hash covered the query (or URL) and options but **not the
organization**, and the search path feeds org-scoped private-corpus providers (the local Tantivy
index and Data Plane retrieval). One organization's private-corpus results, and an answer
synthesized from them, could therefore be served to another organization asking the same
question within the entry's lifetime (up to 24 hours).

The fixed gateway (WS4.A, commit `c207b039`) keys the entries
`verevon:gw:{namespace}:o:{org_id}:{sha256}` and bypasses the cache for ZDR organizations.
Old keys have no `:o:` segment and are never read again by the fixed gateway, but they sit in Redis
until they expire.

## Exposure depends on one setting

The cache exists only when `GATEWAY_CACHE_REDIS_URL` is set on the gateway container. The
canonical compose files do not set it. The only checked-in place that does is the Data Plane
end-to-end stack, `apps/Data Plane v2/tests/e2e/isolated/real-authority-browser-compose.yml`.

## Step 0: which gateways have the cache (run per environment, before deploying WS4.A)

```bash
docker inspect <gateway container> --format '{{range .Config.Env}}{{println .}}{{end}}' \
  | grep -c '^GATEWAY_CACHE_REDIS_URL='          # 1 = cache on, 0 = off (never print the value)
curl -s http://<gateway>:3185/version             # revision and build date
```

Then read the deployment history of that environment's env file: since which revision, and was
the variable ever set earlier than the current container.

| env | `GATEWAY_CACHE_REDIS_URL` | revision / image | cache on since | result |
|---|---|---|---|---|
| local Docker stack (`verevon-gateway-rs`, checked 2026-10-16) | not set (count 0) | `/version` reports `unverified` (image `frontend-plane-verevonv3-gateway`, created 2026-10-05) | never, from the container env | not reachable in this environment |
| Data Plane e2e stack (`real-authority-browser-compose.yml`) | set by the compose file | built per run | every run | exposed during e2e runs only; throw-away data |
| staging / production | **to fill in by the operator** | | | |

## Step 1: read-only forensics (per Redis that has the cache)

```bash
REDIS_CLI="redis-cli -h <host> -a <password> -n <db>" \
  "apps/Frontend Plane/verevonv3/scripts/gateway-cache-forensics.sh"
```

It reports, per namespace, the number of org-scoped entries and legacy entries, and for the
legacy ones: how many carry an `answer`, how many contain a result from the `tantivy_local`
provider (the private corpus), an age histogram and the provider set. It prints numbers only,
never keys, URLs, titles, snippets or answers. Attach its output to the incident record.

## Step 2: hit rate

`redis-cli INFO stats` gives `keyspace_hits` and `keyspace_misses` for the whole instance (coarse,
because rate-limit and session keys share it). A tighter window: compare the number of
`POST /api/v1/search/web` lines in the frontend nginx access log with the `search.upstream`
events in the Quarry edge log for the same window; the difference is the number served from cache.

## What can and cannot be concluded

Entries carry no organization tag and a hit is not logged, so whether one organization actually
received another's results **cannot be answered from the cache** after the fact. The record has to
say one of two things, from the deployment history in step 0 and not from the code:

- "reachable code, never reachable in this environment" (the cache was never enabled), or
- "exposure window up to 24 hours of entries at any time since the cache was enabled; cross-org
  serving can be neither proven nor disproven; treat org-derived entries as potentially served".

## Step 3: deploy WS4.A to every replica

The purge only holds once no replica can write legacy keys again.

## Step 4: purge

```bash
REDIS_CLI="redis-cli -h <host> -a <password> -n <db>" \
  "apps/Frontend Plane/verevonv3/scripts/gateway-cache-purge.sh"            # dry run, prints counts
REDIS_CLI="..." "apps/Frontend Plane/verevonv3/scripts/gateway-cache-purge.sh" --apply
```

It removes only `verevon:gw:search-web:*` and `verevon:gw:scrape-preview:*` keys without `:o:`,
in batches of 100 with `UNLINK`. Never use `FLUSHDB` or `FLUSHALL`: the instance can be shared
with the Data Plane, and database 4 also holds rate-limit and session-validation keys.

## Step 5: verify

Re-run the purge without `--apply` (legacy count 0), re-run the forensics (org-scoped count
unchanged), and check `gateway_result_cache_total` shows `store` and `hit` for the new keys.

## Self-test of the scripts

`apps/Frontend Plane/verevonv3/scripts/tests/gateway-cache-scripts.test.sh` runs against a scratch
Redis (it refuses a non-empty database): forensics reports aggregates only, dry run changes
nothing, `--apply` removes legacy keys (more than one batch of 100), and the org-scoped key and a
neighbouring rate-limit key survive.

```bash
docker run -d --name scratch-redis redis/redis-stack-server:7.4.0-v3
REDIS_CLI="docker exec -i scratch-redis redis-cli" \
  bash "apps/Frontend Plane/verevonv3/scripts/tests/gateway-cache-scripts.test.sh"
```
