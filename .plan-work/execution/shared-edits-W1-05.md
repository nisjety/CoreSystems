## WS01-S7.code
shared-edit apps/Model Plane/docs/ZDR.md: line 407 (Z-60 row, section 4 table) change 'Not run' to 'Pass 1 (fail-closed turn, zero row delta) run <date>, evidence docs/compliance/evidence/live-reads-zdr-<date>.txt; pass 2 (answer from an attested deployment) waits for Z-40' only after the live run passes; also point Step 5 (line ~252) at runbooks/zdr-attestation-and-live-proof.md.

## WS01-S7.code
shared-edit apps/Model Plane/docs/runbooks/README.md: add a line linking zdr-attestation-and-live-proof.md.

## WS01-S7.code
shared-edit docs/compliance/evidence/live-reads-zdr-<date>.txt: created by zdr-live-proof.sh at run time; append the Playwright report path and seed status lines by hand, then commit.

