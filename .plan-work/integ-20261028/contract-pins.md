# Group: cross-plane token contract pins and zdr-contract-sync

## cross-plane-token-requests.json (both copies, no conflict)
Branch added the browser-grant-validate row. Live work filled used_by for web-rerank
(rerank.rs), dropped its "pending" field and extended its notes. Git merged both cleanly.
Both copies are byte-identical (6365 bytes, LF), valid JSON, hold the union. Not edited.
Both called rows exist in source (from_manifest("web-rerank") in rerank.rs,
from_manifest("browser-grant-validate") in grpc/browser_broker.rs).

## The two .sha256 pins
Both sides had a different digest (branch 30ec31e3..., live 40bd3586...). Neither matches
the merged file. Regenerated with sha256sum of the merged JSON, format
"<hash>  cross-plane-token-requests.json", LF: f73f86a2bf1e4d6e891a791ad18b9f062105f24de59c0887920d56892f4954df.
Same bytes written to the Quarry copy and the Auth Core copy. Staged.

## scripts/tests/zdr-contract-sync.test.sh (5 conflicts, kept both sides)
1. Z-05a refusal catalogue comment: kept the branch text (more complete: says it was vendored
   from model-plane 5ae7fe13). Golden row is common.
2. GOLDENS: kept all branch rows (ZDR matrix, tombstone, secret shapes, web-policy catalogue,
   telemetry) and all live rows (QB-T10 retrieve-v1, QB-T09 rerank-v1, QB-M usage ledger).
   The cross-plane row now pins the new digest f73f86a2.... (live's row dropped, same file).
3. VENDORED: kept branch rows (tombstone, refusals) and live rows (model-gateway claims and
   posture matrix copies Z-11, Z-15).
4. VENDORED: kept the branch telemetry copy and live's retrieve-v1, rerank-v1, lab rerank and
   usage-ledger copies.
5. CONSUMERS: kept branch rows (secret shapes mp-scrub, telemetry quarryotel) and live rows
   (model-gateway Z-01, retrieve-v1, rerank-v1). Live's Z-05b "spa" row was a duplicate of a
   row already in the common text, so only one remains.
No defaults involved. LF file, kept LF. bash -n passes. Staged.

## Checks
- scripts/tests/zdr-contract-sync.test.sh: 106 passed, 2 failed. Both failures are NOT merge
  problems: the pin files for telemetry-field-contract.json and usage-envelopes.v2.json are
  CRLF in this worktree's working copy (git index has LF, "i/lf w/crlf"), so the read of the
  digest carries a trailing CR. Digests are equal. Not in my group, not modified; they pass
  from the committed LF content. Worth a glance after commit (git add --renormalize is not
  needed, index is already LF).
- node --test scripts/tests/browser-grant-contract.test.mjs: 32/32 pass (it checks the
  manifest row and that the vendored copy is the same bytes as its pin).
- auth-core jest spec (plane-service-principals-policy.contract.spec.ts): not run, no
  node_modules checked for this worktree's auth-core.
