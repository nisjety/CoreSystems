#!/usr/bin/env bash
# Live proof of the batch pipeline (CR-T8): write-ahead context, vector coherence, reuse by
# input hash, the inline time box. It runs against the LIVE stack, after the embedding-engine
# image carrying services/embedding-engine-rs/src/batch (contexts.rs, store.rs) is deployed
# through the launcher stage copies (never a hand-built `docker compose`) and the migration
# 20261003130000_context_provenance is applied.
#
# SPEND. The ingest is the orchestrator's (the same synthetic document of the CR-T7 proof, three
# chunks, against a throwaway organization): three paid gpt-4o-mini calls, about 0.002 USD, shared
# with CR-T7 and CR-T10. Everything this script does itself is read-only EXCEPT the `reembed`
# step, which replays `knowledge.units.created` for the one document through `reembed-switch`:
# that makes Cohere EMBED calls (one per chunk, so three) and ZERO language-model calls. The
# assertion of the step is exactly that: zero new LLM calls, not zero embed calls. State the
# number of embed calls in the proof report.
#
# Nothing here prints a secret: no environment of any container is printed except the
# non-secret CONTEXT_* / CONTEXTUAL_RETRIEVAL_* settings by name and value, and no connection
# string is built. Do not add `set -x`.
#
# The three steps, in order (each needs CR_LIVE_ORG_ID; the last two need CR_LIVE_DOCUMENT_ID):
#
#   CR_LIVE_STEP=preflight CR_LIVE_ORG_ID=<throwaway org> bash tests/scripts/contextual-batch-live-proof.sh
#       FREE and read-only. The schema has every column the pipeline writes, the migration is
#       recorded, the engine booted with contextual retrieval enabled, and logged no schema
#       refusal. Prints the effective inline settings. Run it before ingesting anything.
#
#   (the orchestrator ingests the synthetic document through documents-api and waits for it to
#    be `indexed`; it records the document id)
#
#   CR_LIVE_STEP=verify CR_LIVE_ORG_ID=<org> CR_LIVE_DOCUMENT_ID=<doc> bash tests/scripts/contextual-batch-live-proof.sh
#       Read-only. Pass criteria, each asserted (the script ends "PASS: batch pipeline verify"):
#         V1  every unit of the document is embedding_status `done`, context_status `ready`;
#         V2  provenance is complete on every unit: a 64-hex context_input_hash, context_model,
#             context_provider, context_residency, context_prompt_version, context_generated_at;
#         V3  coherence: embedded_context_hash equals sha256 of the stored chunk_context on
#             every unit, embedding_model is set, and v_context_coverage says vector_incoherent
#             = 0 and status_unknown = 0 for the organization;
#         V4  embedded_at = updated_at on every unit (nothing wrote the unit after it was marked
#             done, so the stale-embedding detector sees no change);
#         V5  the keyword column matches the context: content_tsv matches the context terms of
#             every unit (the lexical half of contextual retrieval);
#         V6  the Qdrant payload is the one the retrieval engine reads: each point carries exactly
#             knowledge_id, document_id, org_id, chunk_index, text (and document_date when the
#             document has one), and `text` is the chunk's own text, not the composed string.
#             The engine of QB-M1b also stamps `source`, equal to documents.source of the
#             document: a point that carries it must carry the right one, and with
#             CR_LIVE_REQUIRE_SOURCE=1 (the QB-M1b proof) a point without it fails. Without that
#             variable a point without `source` passes, which is the engine of CR-T8 alone;
#         V7  Quickwit holds a context_body for every unit of the document (the keyword index
#             was fed by documents.indexed AFTER the contexts were written);
#         V8  the engine logged the batch with provider_calls equal to the number of chunks
#             (three), and contexts_generated equal to it.
#
#   CR_LIVE_STEP=reembed CR_LIVE_ORG_ID=<org> CR_LIVE_DOCUMENT_ID=<doc> bash tests/scripts/contextual-batch-live-proof.sh
#       WRITES: replays the document's units through the pipeline (reembed-switch, dry run first).
#         R1  before: a snapshot of each unit's chunk_context, context_input_hash,
#             context_generated_at, context_status;
#         R2  the replay is logged by the engine as a batch with provider_calls=0 and
#             contexts_kept equal to the number of chunks, contexts_generated=0;
#         R3  after: every unit's chunk_context, context_input_hash, context_generated_at and
#             context_status are IDENTICAL to the snapshot (no context was rewritten), and
#             embedded_at moved forward (the vectors were really built again);
#         R4  embedded_context_hash is still the hash of the stored context on every unit;
#         R5  if the inference-core counters exist (CR-T3), inference_requests_total for the
#             service embedding-engine did not change across the replay (zero LLM calls
#             measured at the provider boundary, independent of the engine's own log).
#
# Variables
#   CR_LIVE_STEP            preflight | verify | reembed   (required)
#   CR_LIVE_ORG_ID          required
#   CR_LIVE_DOCUMENT_ID     required for verify and reembed
#   EXPECTED_CHUNKS         the number of chunks of the synthetic document (default 3)
#   CR_LIVE_REQUIRE_SOURCE  1 makes V6 fail on a point without the `source` stamp (default 0)
#   POSTGRES_CONTAINER      default data-plane-v2-postgres-1      (database dataplane, role dataplane)
#   ENGINE_CONTAINER        default data-plane-v2-embedding-engine-1
#   INDEX_ENGINE_CONTAINER  default data-plane-v2-index-engine-1
#   QDRANT_CONTAINER        default data-plane-v2-qdrant-1
#   QUICKWIT_CONTAINER      default data-plane-v2-quickwit-1
#   INFERENCE_METRICS_URL   default http://127.0.0.1:18082/metrics (the host port of inference-core)
#   REEMBED_WAIT_SECONDS    how long to wait for the replay to be embedded (default 120)
set -euo pipefail
set +x

STEP="${CR_LIVE_STEP:?set CR_LIVE_STEP to preflight, verify or reembed}"
ORG="${CR_LIVE_ORG_ID:?set CR_LIVE_ORG_ID to a throwaway organization id}"
DOC="${CR_LIVE_DOCUMENT_ID:-}"
CHUNKS="${EXPECTED_CHUNKS:-3}"
PG="${POSTGRES_CONTAINER:-data-plane-v2-postgres-1}"
ENGINE="${ENGINE_CONTAINER:-data-plane-v2-embedding-engine-1}"
INDEXER="${INDEX_ENGINE_CONTAINER:-data-plane-v2-index-engine-1}"
QDRANT="${QDRANT_CONTAINER:-data-plane-v2-qdrant-1}"
QUICKWIT="${QUICKWIT_CONTAINER:-data-plane-v2-quickwit-1}"
METRICS="${INFERENCE_METRICS_URL:-http://127.0.0.1:18082/metrics}"
WAIT="${REEMBED_WAIT_SECONDS:-120}"

fail() { printf 'FAIL: %s\n' "$*" >&2; exit 1; }
pass() { printf 'PASS: %s\n' "$*"; }
note() { printf '      %s\n' "$*"; }

[[ $ORG =~ ^[A-Za-z0-9][A-Za-z0-9._:-]{0,127}$ ]] || fail "CR_LIVE_ORG_ID is not a valid organization id"
[[ -z $DOC || $DOC =~ ^[A-Za-z0-9][A-Za-z0-9._:-]{0,127}$ ]] || fail "CR_LIVE_DOCUMENT_ID is not a valid document id"
for container in "$PG" "$ENGINE"; do
  docker inspect "$container" >/dev/null 2>&1 || fail "container $container is not running"
done

# One read-only psql call. The SQL goes in on stdin because psql does not substitute variables in
# a -c string; -v passes the ids as variables (:'org'), never spliced into the SQL text.
psql_q() {
  printf '%s\n' "$1" | docker exec -i "$PG" psql -U dataplane -d dataplane -X -q -tA -F '|' \
    -v ON_ERROR_STOP=1 -v org="$ORG" -v doc="$DOC"
}

need_doc() { [[ -n $DOC ]] || fail "CR_LIVE_DOCUMENT_ID is required for the $STEP step"; }

# The engine's batch log lines since a moment, as the numbers they carry. The log may be text
# (key=value) or JSON ("key":value); both are read.
log_number() { # <line> <field>
  printf '%s' "$1" | sed -nE "s/.*[\" ]$2[\"]?[=:] ?\"?([0-9]+).*/\1/p" | head -n1
}
batch_lines_since() { docker logs --since "$1" "$ENGINE" 2>&1 | grep -E 'batch embedded' || true; }

case "$STEP" in
# ─────────────────────────────────────────────────────────────────────────────────────────
preflight)
  missing="$(psql_q "SELECT count(*) FROM unnest(ARRAY['chunk_context','context_prompt_version','context_status','context_model','context_provider','context_residency','context_input_hash','embedded_context_hash','context_generated_at','context_attempts','context_next_attempt_at','context_error_class','embedding_model']) AS c(name) WHERE NOT EXISTS (SELECT 1 FROM information_schema.columns WHERE table_schema = 'public' AND table_name = 'knowledge_units' AND column_name = c.name)")"
  [[ $missing == 0 ]] || fail "knowledge_units lacks $missing of the 13 columns the pipeline writes: apply migration 20261003130000_context_provenance first"
  pass "knowledge_units has every column the pipeline writes"
  recorded="$(psql_q "SELECT count(*) FROM schema_migrations WHERE version = '20261003130000'")"
  [[ $recorded == 1 ]] || fail "migration 20261003130000 is not recorded in schema_migrations"
  pass "migration 20261003130000_context_provenance is recorded"
  logs="$(docker logs --tail 600 "$ENGINE" 2>&1 || true)"
  grep -q 'contextual retrieval ENABLED' <<<"$logs" || fail "the engine did not log 'contextual retrieval ENABLED' at boot (is the new image running, and CONTEXTUAL_RETRIEVAL_ENABLED true?)"
  grep -q 'contexts are generated inline within a time box and reused by input hash' <<<"$logs" || fail "the engine's boot line is the old one: the image does not carry the batch pipeline"
  ! grep -q 'knowledge_units has no column' <<<"$logs" || fail "the engine refused the schema at boot: apply the migration and restart"
  pass "the engine booted with the batch pipeline and found its schema"
  note "effective settings (names and values only):"
  docker exec "$ENGINE" printenv | grep -E '^(CONTEXT_INLINE_|CONTEXTUAL_RETRIEVAL_(ENABLED|MODEL|CONCURRENCY|PROVIDER_HINT|MIN_RESIDENCY|PROMPT_VERSION|RETRY_))' | sort | sed 's/^/        /' || true
  note "CONTEXT_INLINE_DOCUMENT_CONCURRENCY unset means the default (2); CONTEXT_INLINE_BUDGET_MS 20000 and CONTEXT_INLINE_DOCUMENT_BUDGET_MS 8000 are the defaults."
  pass "preflight"
  ;;

# ─────────────────────────────────────────────────────────────────────────────────────────
verify)
  need_doc
  row="$(psql_q "
    SELECT count(*),
           count(*) FILTER (WHERE embedding_status = 'done'),
           count(*) FILTER (WHERE context_status = 'ready'),
           count(*) FILTER (WHERE context_input_hash ~ '^[0-9a-f]{64}\$' AND context_model IS NOT NULL
                              AND context_provider IS NOT NULL AND context_residency IS NOT NULL
                              AND context_prompt_version IS NOT NULL AND context_generated_at IS NOT NULL),
           count(*) FILTER (WHERE embedded_context_hash = context_text_hash(chunk_context)),
           count(*) FILTER (WHERE embedding_model IS NOT NULL),
           count(*) FILTER (WHERE embedded_at = updated_at),
           count(*) FILTER (WHERE content_tsv @@ plainto_tsquery('simple', chunk_context))
      FROM knowledge_units WHERE org_id = :'org' AND document_id = :'doc'")"
  IFS='|' read -r units done ready provenance coherent modeled settled lexical <<<"$row"
  [[ $units == "$CHUNKS" ]] || fail "the document has $units units, expected $CHUNKS (EXPECTED_CHUNKS)"
  [[ $done == "$units" && $ready == "$units" ]] || fail "V1: $done done and $ready ready of $units units"
  pass "V1: every unit is done and ready"
  [[ $provenance == "$units" ]] || fail "V2: provenance is complete on $provenance of $units units"
  pass "V2: the provenance is complete on every unit"
  [[ $coherent == "$units" && $modeled == "$units" ]] || fail "V3: $coherent coherent and $modeled with embedding_model of $units units"
  coverage="$(psql_q "SELECT vector_incoherent, status_unknown FROM v_context_coverage WHERE org_id = :'org'")"
  [[ $coverage == "0|0" ]] || fail "V3: v_context_coverage says vector_incoherent|status_unknown = $coverage"
  pass "V3: every vector says which context it was built from, and the coverage view reads 0 incoherent, 0 unknown"
  [[ $settled == "$units" ]] || fail "V4: embedded_at = updated_at on only $settled of $units units (something wrote a unit after it was marked done)"
  pass "V4: nothing wrote a unit after it was marked done"
  [[ $lexical == "$units" ]] || fail "V5: content_tsv matches the context on $lexical of $units units"
  pass "V5: the keyword column matches the context of every unit"

  collection="$(docker exec "$ENGINE" printenv QDRANT_COLLECTION)"
  docsource="$(psql_q "SELECT source FROM documents WHERE org_id = :'org' AND document_id = :'doc'")"
  [[ -n $docsource ]] || fail "V6: the document $DOC has no row in documents under the organization $ORG"
  ids="$(psql_q "SELECT knowledge_id || '|' || chunk_index || '|' || md5(text) FROM knowledge_units WHERE org_id = :'org' AND document_id = :'doc' ORDER BY chunk_index")"
  while IFS='|' read -r kid index textmd5; do
    [[ -n $kid ]] || continue
    body="$(docker exec "$QDRANT" bash -c "exec 3<>/dev/tcp/127.0.0.1/6333; printf 'GET /collections/$collection/points/$kid HTTP/1.0\r\nHost: localhost\r\n\r\n' >&3; cat <&3" | sed '1,/^\r\{0,1\}$/d')"
    keys="$(printf '%s' "$body" | node -e '
      let s = ""; process.stdin.on("data", (d) => (s += d)).on("end", () => {
        const point = JSON.parse(s).result;
        if (!point || !point.payload) { console.log("NO-POINT"); return; }
        console.log(Object.keys(point.payload).sort().join(","));
      });')"
    stamped=""
    case "$keys" in
      "chunk_index,document_id,knowledge_id,org_id,text" | "chunk_index,document_date,document_id,knowledge_id,org_id,text") ;;
      "chunk_index,document_id,knowledge_id,org_id,source,text" | "chunk_index,document_date,document_id,knowledge_id,org_id,source,text") stamped=1 ;;
      *) fail "V6: the payload of $kid has keys [$keys]" ;;
    esac
    if [[ -n $stamped ]]; then
      pointsource="$(printf '%s' "$body" | node -e '
        let s = ""; process.stdin.on("data", (d) => (s += d)).on("end", () => {
          console.log(JSON.parse(s).result.payload.source);
        });')"
      [[ $pointsource == "$docsource" ]] || fail "V6: the source in the point of $kid is [$pointsource], documents.source is [$docsource]"
    elif [[ ${CR_LIVE_REQUIRE_SOURCE:-0} == 1 ]]; then
      fail "V6: the point of $kid carries no source (CR_LIVE_REQUIRE_SOURCE=1)"
    fi
    pointmd5="$(printf '%s' "$body" | node -e '
      const crypto = require("crypto");
      let s = ""; process.stdin.on("data", (d) => (s += d)).on("end", () => {
        console.log(crypto.createHash("md5").update(JSON.parse(s).result.payload.text, "utf8").digest("hex"));
      });')"
    [[ $pointmd5 == "$textmd5" ]] || fail "V6: the text in the point of $kid is not the chunk's own text"
  done <<<"$ids"
  pass "V6: the Qdrant payload is the one the retrieval engine reads (the keys, the chunk's own text, and the source where it is stamped)"

  hits="$(docker exec "$QUICKWIT" curl -s -X POST -H 'content-type: application/json' \
    http://localhost:7280/api/v1/dataplane-corpus/search \
    -d "{\"query\":\"entity_type:knowledge_unit AND document_id:$DOC AND context_body:*\",\"max_hits\":0}" |
    sed -nE 's/.*"num_hits":([0-9]+).*/\1/p')"
  [[ ${hits:-0} -ge $CHUNKS ]] || fail "V7: Quickwit has $hits context_body documents for the document, expected at least $CHUNKS (the adapter may still be indexing; retry in a minute)"
  pass "V7: Quickwit holds a context_body for every unit of the document ($hits hit(s); a delete is eventual, so a count above $CHUNKS before the merge is expected)"

  line="$(docker logs "$ENGINE" 2>&1 | grep -E 'batch embedded' | grep -F "$DOC" | head -n1 || true)"
  [[ -n $line ]] || fail "V8: no 'batch embedded' line names the document $DOC in the engine log (is RUST_LOG at least embedding_engine=info?)"
  calls="$(log_number "$line" provider_calls)"
  generated="$(log_number "$line" contexts_generated)"
  [[ ${calls:-x} == "$CHUNKS" && ${generated:-x} == "$CHUNKS" ]] || fail "V8: the batch logged provider_calls=${calls:-?} contexts_generated=${generated:-?}, expected $CHUNKS each"
  pass "V8: the batch made exactly $CHUNKS provider calls and generated $CHUNKS contexts"
  note "input/output tokens of that batch, for the report: $(printf '%s' "$line" | grep -oE '(input_tokens|output_tokens|cache_read_input_tokens)[=\":]+ ?[0-9]+' | tr '\n' ' ')"
  pass "batch pipeline verify"
  ;;

# ─────────────────────────────────────────────────────────────────────────────────────────
reembed)
  need_doc
  docker inspect "$INDEXER" >/dev/null 2>&1 || fail "container $INDEXER is not running"
  snapshot() {
    psql_q "SELECT md5(knowledge_id || '|' || coalesce(chunk_context,'') || '|' || coalesce(context_input_hash,'') || '|' || coalesce(context_generated_at::text,'') || '|' || coalesce(context_status,'')) FROM knowledge_units WHERE org_id = :'org' AND document_id = :'doc' ORDER BY knowledge_id"
  }
  embedded_max() { psql_q "SELECT coalesce(extract(epoch FROM max(embedded_at))::text, '0') FROM knowledge_units WHERE org_id = :'org' AND document_id = :'doc'"; }
  llm_counter() {
    curl -s --max-time 5 "$METRICS" 2>/dev/null | grep -E '^inference_requests_total' | grep -F 'embedding-engine' |
      sed -E 's/.* ([0-9.]+)$/\1/' | paste -sd+ - | bc 2>/dev/null || true
  }
  before="$(snapshot)"
  [[ $(printf '%s\n' "$before" | grep -c .) == "$CHUNKS" ]] || fail "R1: the document has $(printf '%s\n' "$before" | grep -c .) units, expected $CHUNKS"
  pass "R1: snapshot of the contexts taken ($CHUNKS units)"
  moved_before="$(embedded_max)"
  counter_before="$(llm_counter)"
  started="$(date -u +%Y-%m-%dT%H:%M:%SZ)"

  note "dry run:"
  docker exec -e RUST_LOG=warn "$INDEXER" reembed-switch --org-id "$ORG" --document-id "$DOC" --dry-run | sed 's/^/        /'
  note "replaying the document's units (Cohere embed calls: $CHUNKS; language-model calls: expected 0):"
  docker exec -e RUST_LOG=warn "$INDEXER" reembed-switch --org-id "$ORG" --document-id "$DOC" | sed 's/^/        /'

  deadline=$((SECONDS + WAIT))
  until [[ $(embedded_max) != "$moved_before" ]] || (( SECONDS >= deadline )); do sleep 2; done
  [[ $(embedded_max) != "$moved_before" ]] || fail "R3: the units were not embedded again within ${WAIT}s"
  sleep 3
  pass "R3: the vectors were built again (embedded_at moved forward)"

  lines="$(batch_lines_since "$started")"
  [[ -n $lines ]] || fail "R2: the engine logged no 'batch embedded' line since $started"
  total_calls=0; total_kept=0; total_generated=0
  while IFS= read -r line; do
    [[ -n $line ]] || continue
    total_calls=$((total_calls + $(log_number "$line" provider_calls)))
    total_kept=$((total_kept + $(log_number "$line" contexts_kept)))
    total_generated=$((total_generated + $(log_number "$line" contexts_generated)))
  done <<<"$lines"
  [[ $total_calls == 0 && $total_generated == 0 ]] || fail "R2: the replay made $total_calls provider calls and generated $total_generated contexts"
  [[ $total_kept == "$CHUNKS" ]] || fail "R2: the replay kept $total_kept contexts, expected $CHUNKS"
  pass "R2: the replay made 0 provider calls and kept $total_kept stored contexts"

  after="$(snapshot)"
  [[ $before == "$after" ]] || fail "R3: a context, its input hash, its time or its status changed: a stored context was rewritten"
  pass "R3: every context, hash, generation time and status is identical to the snapshot"
  incoherent="$(psql_q "SELECT count(*) FROM knowledge_units WHERE org_id = :'org' AND document_id = :'doc' AND embedded_context_hash IS DISTINCT FROM context_text_hash(chunk_context)")"
  [[ $incoherent == 0 ]] || fail "R4: $incoherent units have a vector that does not match their stored context"
  pass "R4: every vector is still built from the stored context"

  counter_after="$(llm_counter)"
  if [[ -n $counter_before && -n $counter_after ]]; then
    [[ $counter_before == "$counter_after" ]] || fail "R5: inference_requests_total for embedding-engine moved from $counter_before to $counter_after"
    pass "R5: inference-core counted no request from embedding-engine across the replay ($counter_after)"
  else
    note "R5: inference-core's per-service counter is not exposed on $METRICS (CR-T3 not deployed); the engine's own count (R2) stands alone"
  fi
  pass "batch pipeline re-embed: 0 language-model calls, $CHUNKS embed calls"
  ;;

*) fail "CR_LIVE_STEP must be preflight, verify or reembed" ;;
esac
