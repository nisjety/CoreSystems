#!/usr/bin/env bash
set -euo pipefail
set +x

ROOT=$(cd "$(dirname "$0")/../.." && pwd)
OVERRIDE="$ROOT/tests/e2e/isolated/docker-compose.yml"

: "${DPV2_COMPOSE_PROJECT:?required}"
: "${DATAPLANE_DRAGONFLY_PASSWORD:?required}"
: "${MINIO_ROOT_USER:?required}"
: "${MINIO_ROOT_PASSWORD:?required}"

case "$DPV2_COMPOSE_PROJECT" in
  dpv2-mvp-e2e-*) ;;
  *) echo "multi-store snapshot requires a disposable dpv2-mvp-e2e project" >&2; exit 2 ;;
esac

for command in docker node openssl rg sed sort tr; do
  command -v "$command" >/dev/null 2>&1 || { echo "missing required command: $command" >&2; exit 2; }
done

readonly -a compose=(docker compose --project-name "$DPV2_COMPOSE_PROJECT" -f "$ROOT/docker-compose.yml" -f "$OVERRIDE")

digest() {
  openssl dgst -sha256 | sed 's/^.*= //'
}

postgres_snapshot() {
  "${compose[@]}" exec -T postgres psql -U dataplane -d dataplane -Atq -c '
    SELECT name || '"'"'='"'"' || rows FROM (
      SELECT '"'"'documents'"'"' AS name, count(*)::bigint AS rows FROM documents
      UNION ALL SELECT '"'"'source_objects'"'"', count(*) FROM source_objects
      UNION ALL SELECT '"'"'knowledge_units'"'"', count(*) FROM knowledge_units
      UNION ALL SELECT '"'"'retrieval_runs'"'"', count(*) FROM retrieval_runs
      UNION ALL SELECT '"'"'retrieval_candidates'"'"', count(*) FROM retrieval_candidates
      UNION ALL SELECT '"'"'graph_entities'"'"', count(*) FROM graph_entities
      UNION ALL SELECT '"'"'graph_relationships'"'"', count(*) FROM graph_relationships
      UNION ALL SELECT '"'"'graph_claims'"'"', count(*) FROM graph_claims
      UNION ALL SELECT '"'"'graph_text_units'"'"', count(*) FROM graph_text_units
      UNION ALL SELECT '"'"'chunk_lineage'"'"', count(*) FROM chunk_lineage
      UNION ALL SELECT '"'"'wiki_pages'"'"', count(*) FROM wiki_pages
      UNION ALL SELECT '"'"'wiki_page_versions'"'"', count(*) FROM wiki_page_versions
      UNION ALL SELECT '"'"'wiki_source_logs'"'"', count(*) FROM wiki_source_logs
      UNION ALL SELECT '"'"'wiki_maintenance_logs'"'"', count(*) FROM wiki_maintenance_logs
      UNION ALL SELECT '"'"'wiki_proposals'"'"', count(*) FROM wiki_proposals
      UNION ALL SELECT '"'"'documents_outbox'"'"', count(*) FROM documents_outbox
      UNION ALL SELECT '"'"'index_deletion_outbox'"'"', count(*) FROM index_deletion_outbox
      UNION ALL SELECT '"'"'wiki_event_outbox'"'"', count(*) FROM wiki_event_outbox
      UNION ALL SELECT '"'"'cost_events'"'"', count(*) FROM cost_events
    ) AS snapshot ORDER BY name;' | digest
}

qdrant_snapshot() {
  local collections names name info
  collections=$("${compose[@]}" exec -T documents-api curl -fsS http://qdrant:6333/collections)
  names=$(COLLECTIONS_JSON="$collections" node -e '
    const body=JSON.parse(process.env.COLLECTIONS_JSON);
    const names=(body.result?.collections ?? []).map((item)=>item?.name).filter((name)=>typeof name==="string" && /^[A-Za-z0-9._-]{1,128}$/.test(name)).sort();
    process.stdout.write(names.join("\n"));
  ')
  {
    while IFS= read -r name; do
      [ -n "$name" ] || continue
      info=$("${compose[@]}" exec -T documents-api curl -fsS "http://qdrant:6333/collections/$name")
      COLLECTION_NAME="$name" COLLECTION_INFO="$info" node -e '
        const body=JSON.parse(process.env.COLLECTION_INFO).result ?? {};
        process.stdout.write(`${process.env.COLLECTION_NAME}:${body.points_count ?? 0}:${body.indexed_vectors_count ?? 0}:${body.segments_count ?? 0}\n`);
      '
    done <<<"$names"
  } | sort | digest
}

dragonfly_snapshot() {
  {
    # Cumulative write-command counters catch transient cache writes that are
    # deleted before the after-snapshot. Read/SCAN counters are intentionally
    # excluded because this metadata-only proof increments them itself.
    "${compose[@]}" exec -T dragonfly redis-cli --no-auth-warning \
      -a "$DATAPLANE_DRAGONFLY_PASSWORD" INFO commandstats 2>/dev/null \
      | tr -d '\r' \
      | rg -i '^cmdstat_(set|setex|psetex|setnx|getset|mset|msetnx|append|incr|incrby|incrbyfloat|decr|decrby|del|unlink|expire|expireat|pexpire|pexpireat|persist|rename|renamenx|hset|hsetnx|hmset|hdel|hincrby|hincrbyfloat|sadd|srem|smove|spop|zadd|zincrby|zrem|zremrangebyrank|zremrangebyscore|zremrangebylex|lpush|rpush|lpop|rpop|lset|ltrim|linsert|lrem|rpoplpush|brpoplpush|lmove|blmove|xadd|xdel|xtrim|copy|restore|migrate|move|flushdb|flushall):' \
      | sort || true
    for database in {0..15}; do
      printf 'db=%s\n' "$database"
      "${compose[@]}" exec -T dragonfly redis-cli --no-auth-warning \
        -a "$DATAPLANE_DRAGONFLY_PASSWORD" -n "$database" --scan 2>/dev/null | sort
    done
  } | digest
}

quickwit_snapshot() {
  local response
  response=$("${compose[@]}" exec -T documents-api curl -fsS \
    -H 'content-type: application/json' \
    --data '{"query":"*","max_hits":0}' \
    http://quickwit:7280/api/v1/dataplane-corpus/search)
  QUICKWIT_RESPONSE="$response" node -e '
    const body=JSON.parse(process.env.QUICKWIT_RESPONSE);
    if (!Number.isSafeInteger(body.num_hits) || (body.hits?.length ?? 0) !== 0) process.exit(1);
    process.stdout.write(`dataplane-corpus:${body.num_hits}\n`);
  ' | digest
}

minio_snapshot() {
  "${compose[@]}" run --rm -T --no-deps \
    -e MINIO_ROOT_USER -e MINIO_ROOT_PASSWORD \
    --entrypoint /bin/sh minio-init -ceu '
      mc alias set snapshot http://minio:9000 "$MINIO_ROOT_USER" "$MINIO_ROOT_PASSWORD" >/dev/null
      for bucket in quickwit dataplane-cas; do
        printf "bucket=%s\n" "$bucket"
        mc ls --recursive --json "snapshot/$bucket"
      done
    ' 2>/dev/null | sort | digest
}

nats_snapshot() {
  local response
  response=$("${compose[@]}" exec -T nats wget -qO- 'http://127.0.0.1:8222/jsz?streams=true&config=true')
  NATS_JSZ="$response" node -e '
    const body=JSON.parse(process.env.NATS_JSZ);
    const streams=(body.account_details ?? []).flatMap((account)=>(account.stream_detail ?? []).map((stream)=>({
      account: account.name ?? "",
      name: stream.name ?? stream.config?.name ?? "",
      firstSequence: stream.state?.first_seq ?? 0,
      lastSequence: stream.state?.last_seq ?? 0,
      messages: stream.state?.messages ?? 0,
      bytes: stream.state?.bytes ?? 0,
      consumers: stream.state?.consumer_count ?? 0,
    }))).sort((a,b)=>`${a.account}:${a.name}`.localeCompare(`${b.account}:${b.name}`));
    for (const stream of streams) process.stdout.write(`${stream.account}:${stream.name}:${stream.firstSequence}:${stream.lastSequence}:${stream.messages}:${stream.bytes}:${stream.consumers}\n`);
  ' | digest
}

{
  printf 'postgres=%s\n' "$(postgres_snapshot)"
  printf 'qdrant=%s\n' "$(qdrant_snapshot)"
  printf 'dragonfly=%s\n' "$(dragonfly_snapshot)"
  printf 'quickwit=%s\n' "$(quickwit_snapshot)"
  printf 'minio=%s\n' "$(minio_snapshot)"
  printf 'nats=%s\n' "$(nats_snapshot)"
} | digest
