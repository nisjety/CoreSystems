#!/bin/bash
# L-SC5-SC6-docs-zdr: ZDR probes on the live edge (RUST_LOG=debug), fixture-a only. No secrets.
cd "$(dirname "$0")"
E=http://127.0.0.1:8082; B="Authorization: Bearer dev-evidence"
cnt() { curl -s -H "$B" "$E/v1/artifacts?limit=200" | jq -c '.data.items|length'; }
newid() { node -e 'console.log(require("crypto").randomUUID())'; }
scan() { # $1 marker $2 since ; prints counts from the edge log window
  sleep 2
  docker logs --since "$2" quarry-edge > "zdr-win-$3.log" 2>&1
  echo "  log lines in window: $(wc -l < zdr-win-$3.log)"
  echo "  marker '$1': $(grep -c "$1" zdr-win-$3.log)"
  echo "  'fixture-a': $(grep -ci 'fixture-a' zdr-win-$3.log)   'crawl.test': $(grep -ci 'crawl\.test' zdr-win-$3.log)   'Fixture A' (title): $(grep -c 'Fixture A' zdr-win-$3.log)"
}
echo "== 1 ZDR scrape =="
Z=$(newid); BEFORE=$(cnt); T=$(date -u +%FT%TZ)
curl -s -o z1.json -w "  HTTP %{http_code}\n" -X POST $E/v1/scrape -H "$B" -H "Content-Type: application/json" -d "{\"url\":\"http://fixture-a.crawl.test/?zdr-probe-$Z\",\"zdr\":true}"
AFTER=$(cnt)
echo "  artifacts before=$BEFORE after=$AFTER"
echo "  inline markdown starts: $(jq -r '.data.markdown|.[0:30]' z1.json | tr '\n' ' ') scrape_id=$(jq -r '.data.scrape_id' z1.json) zdr=$(jq -r '.data.privacy.zdr' z1.json)"
echo "  formats keys: $(jq -c '.data.formats|keys' z1.json)"
scan "zdr-probe-$Z" "$T" z1
echo "== 1c control: zdr false =="
S=$(newid); T=$(date -u +%FT%TZ); B2=$(cnt)
curl -s -o c1.json -w "  HTTP %{http_code}\n" -X POST $E/v1/scrape -H "$B" -H "Content-Type: application/json" -d "{\"url\":\"http://fixture-a.crawl.test/?zdr-probe-$S\",\"zdr\":false}"
echo "  artifacts before=$B2 after=$(cnt)"
scan "zdr-probe-$S" "$T" c1
echo "== 2 ZDR stream =="
Z2=$(newid); T=$(date -u +%FT%TZ); B3=$(cnt)
curl -s -N --max-time 60 -o s1.sse -w "  HTTP %{http_code}\n" -X POST $E/v1/scrape/stream -H "$B" -H "Content-Type: application/json" -d "{\"url\":\"http://fixture-a.crawl.test/?zdr-probe-$Z2\",\"zdr\":true}"
echo "  artifacts before=$B3 after=$(cnt); sse bytes=$(wc -c < s1.sse)"
scan "zdr-probe-$Z2" "$T" s1
echo "== 4 ZDR agent run + navigate =="
Z4=$(newid); T=$(date -u +%FT%TZ); B4=$(cnt)
RID=$(curl -s -X POST $E/v1/agent/runs -H "$B" -H "Content-Type: application/json" -d '{"zdr":true}' | jq -r .data.run_id); echo "  run $RID"
curl -s -o a1.json -w "  navigate HTTP %{http_code}\n" -X POST $E/v1/agent/runs/$RID/step -H "$B" -H "Content-Type: application/json" -d "{\"action\":{\"type\":\"navigate\",\"url\":\"http://fixture-a.crawl.test/?zdr-probe-$Z4\"}}"
curl -s -w "  close HTTP %{http_code}\n" -o /dev/null -X DELETE $E/v1/agent/runs/$RID -H "$B"
echo "  artifacts before=$B4 after=$(cnt)"
scan "zdr-probe-$Z4" "$T" a1
