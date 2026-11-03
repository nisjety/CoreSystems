#!/bin/bash
# $1 = tag. Agent-labelled run; password via selector naming type=password, then via click_ref; search box via click_ref.
cd "$(dirname "$0")"
TAG=$1
E=http://127.0.0.1:8082; B="Authorization: Bearer dev-evidence"; J="Content-Type: application/json"
step() { curl -s -o step-last-$TAG.json -w "%{http_code}" -X POST $E/v1/agent/runs/$RID/step -H "$B" -H "$J" -d "$1"; }
A='"initiated_by":"agent"'
presses() { local s="$1"; for ((i=0;i<${#s};i++)); do c="${s:$i:1}"; echo -n "$c:$(step "{$A,\"action\":{\"type\":\"press\",\"key\":\"$c\"}}") "; done; echo; }
clickref() { # $1 = accessible name prefix
  local snap gen ref
  snap=$(jq -r .data.snapshot.snapshot_id step-last-$TAG.json); gen=$(jq -r .data.snapshot.generation step-last-$TAG.json)
  ref=$(jq -r --arg n "$1" '.data.snapshot.targets[]|select(.name|startswith($n))|.ref_id' step-last-$TAG.json | head -1)
  echo -n "click_ref $1 ($ref gen $gen): "; step "{$A,\"action\":{\"type\":\"click_ref\",\"snapshot_id\":\"$snap\",\"generation\":$gen,\"ref_id\":\"$ref\"}}"; echo
}
RID=$(curl -s -X POST $E/v1/agent/runs -H "$B" -H "$J" -d "{\"zdr\":false,$A}" | jq -r .data.run_id); echo "run $RID"; echo $RID > x19$TAG-run-id.txt
date -u +%FT%TZ > x19$TAG-t0.txt
echo "navigate: $(step "{$A,\"action\":{\"type\":\"navigate\",\"url\":\"http://forms.qlive.crawl.test/login.html\"}}")"
echo "--- P1: click selector input[type=password], 10 keys, Enter"
echo "click: $(step "{$A,\"action\":{\"type\":\"click\",\"selector\":\"input[type=password]\"}}")"
presses qlivepass7
echo "Enter: $(step "{$A,\"action\":{\"type\":\"press\",\"key\":\"Enter\"}}")"
echo "--- P2: click_ref password, 10 keys, Enter"
clickref Password
presses qlivepass7
echo "Enter: $(step "{$A,\"action\":{\"type\":\"press\",\"key\":\"Enter\"}}")"
echo "--- S: click_ref search box, 9 keys, Enter"
clickref Search
presses zebrafish
echo "Enter: $(step "{$A,\"action\":{\"type\":\"press\",\"key\":\"Enter\"}}")"
echo "--- T: type_ref into search box and into password"
clickref Search
SNAP=$(jq -r .data.snapshot.snapshot_id step-last-$TAG.json); GEN=$(jq -r .data.snapshot.generation step-last-$TAG.json)
REF=$(jq -r '.data.snapshot.targets[]|select(.name|startswith("Search"))|.ref_id' step-last-$TAG.json|head -1)
echo "type_ref search: $(step "{$A,\"action\":{\"type\":\"type_ref\",\"snapshot_id\":\"$SNAP\",\"generation\":$GEN,\"ref_id\":\"$REF\",\"text\":\"pelican\"}}")"
echo "close: $(curl -s -o /dev/null -w "%{http_code}" -X DELETE $E/v1/agent/runs/$RID -H "$B")"
