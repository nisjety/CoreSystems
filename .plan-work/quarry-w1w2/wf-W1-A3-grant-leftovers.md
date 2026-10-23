# W1-A3-grant-leftovers hand-over (2026-10-23)

Done (commits on quarryv2-next):
- d7a04e07 broker per-call log line (grantlog.go + tests; adopted from an earlier cut-off agent's uncommitted files, tests re-run: go test ./internal/server ok)
- 3b13119e compose lines (BROWSER_BROKER_MAX_RUN_DOMAINS in Model Plane compose, BROWSER_GRANT_ENFORCEMENT empty default in the Frontend compose) + L1 tests (27 pass)
- d6b273e6 ledger: broker restart semantics table and recipe
- 02cbd74c ledger: WS1.A1 entry (D1, D2, D5, D6, R-6)
Already present, not rebuilt: base Ingestion compose validator line (docker-compose.yml:139), ZDR log assertion (in grantlog_test.go).
Errata text: A3-ledger-text.md. Depth bullets: depth/W1-A3-grant-leftovers.md.
Traps: the plan said the two env vars belong in "Model Plane and Ingestion compose"; BROWSER_GRANT_ENFORCEMENT is read by the Frontend gateway, so it went into the Frontend compose. The ledger is CRLF: appended with a CRLF-preserving script.
