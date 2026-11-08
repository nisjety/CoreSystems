## WS04-S3
shared-edit apps/Model Plane/api/contracts/zdr-posture-matrix.json (+ .sha256, the pin in scripts/tests/zdr-contract-sync.test.sh, the three vendored copies under mp-contracts, model-gateway and execution-core tests/fixtures, and the loader zdr-contract-schema.mjs if it needs the new shape): the enforcement row e_zdr_token_queue_input_refused, once the integrator decides the row shape for an operation refusal.

## WS04-S3
shared-edit apps/Model Plane/docs/ZDR.md and apps/Model Plane/MODEL_PLANE_STATUS.md at WS04-S9: queued text is never put on the bus for ZDR or waived tokens; queue and probe ops are on the same MODEL_GATEWAY_RUN_CONTROL_BUS switch. Also gateway_metrics.rs could take a queued_input_undelivered helper beside run_control_request (today sse.rs calls metrics::counter! directly, as other counters in sse.rs do).

## WS06-S9
shared-edit docs/compliance/evidence/tools/scan-egress.mjs (LF): insert after the okta-oidc entry, before the line `  {` that opens the cloudflare-turnstile entry:
  {
    id: 'customer-siem-destination',
    name: "Customer-configured SIEM destination (audit export to the customer's own endpoint)",
    class: 'user-authorised',
    role: 'customer-directed',
    hosts: [],
    gateEnv: ['AUDIT_SIEM_EXPORT_ENABLED', 'ORG_SIEM_DESTINATIONS_ENABLED'],
    note: 'The URL is the one an organisation owner or admin configures, so no literal exists. Audit rows of that organisation leave for it only when the exporter and the destination are both switched on.',
  },

## WS06-S9
shared-edit docs/compliance/evidence/subprocessors-from-code.md (CRLF): insert one line after the row that starts with | `crm-erp-import-sources` (the register is ordered by id):
| `customer-siem-destination` | Customer-configured SIEM destination (audit export to the customer's own endpoint) | user-authorised; customer-directed | Audit rows of the customer's organisation, in chain order, to the URL its owner or admin configures, when the exporter and the destination are both switched on: the stored envelope fields (user id, role, plane, event, subject, resource id, outcome, request id, IP host, user agent, source producer and bus, details, hashes). The identifiers, the IP host and the user agent are personal data. | `AUDIT_SIEM_EXPORT_ENABLED` is read in the siem-exporter; `ORG_SIEM_DESTINATIONS_ENABLED` is read in org-core. Not read on the dev stack. Both default to 0 in the compose file. | Pack: no. Trust page: no. | EXCLUDED: the customer's own system, configured by the customer; disclose as a recipient class, not a sub-processor; counsel to confirm | `apps/Control Plane/audit-core/cmd/siem-exporter/main.go:56 @"AUDIT_SIEM_EXPORT_ENABLED"`<br>`apps/Control Plane/org-core/internal/http/siem_handlers.go:18 @"ORG_SIEM_DESTINATIONS_ENABLED"` |

## WS06-S9
shared-edit apps/Frontend Plane/verevonv3/apps/verevon-web/src/components/trust/trust-recipients.ts (LF; NOT in the S9 notes, but trust-data.test.ts 'accounts for every vendor the scan finds' fails without it): insert after the crm-erp-import-sources entry (its closing `},` line) and before the discord-api entry:
	{
		id: "customer-siem-destination",
		status: "excluded",
		reason: "the customer's own SIEM, configured by the customer's owner or admin; the register classes the export as customer-directed, for counsel to confirm",
	},

## WS06-S9
shared-edit apps/Model Plane/docs/ZDR.md:95 (CRLF): replace the whole row that starts with | **NATS / audit envelopes** with:
| **NATS / audit envelopes** | ZDR-derived content not published to durable subjects | Built. `zdr: true` envelopes are suppressed by natsx. The audit chain and its SIEM export carry the audit envelope only. Producers keep `details` content-free by contract, and audit-core counts oversized or denied `details` keys without rejecting them (`audit_core_details_guard_total`). |

## WS06-S9
shared-edit docs/compliance/evidence/honest-limits.md (CRLF), part a: insert after the L35 bullet (the last bullet of section 7, before '## 8. What Verevon does not have'):
- **L37. Audit rows are chained only where the chain is switched on, and the chain does not prove integrity against a database owner acting alone.** The chain is off unless `AUDIT_CHAIN_ENABLED` is set to `1`. Where it is on, the signing key sits outside the database, but the rows, their hashes and the attestations sit inside it. A database owner can rewrite rows and delete the attestations that cover them, and the rewritten chain then recomputes cleanly and is not reported as broken. Detection needs a signed head or checkpoint that covers the rewritten rows and is held outside the database, such as a SIEM copy or a customer export taken earlier.
- **L38. Completeness is not proven.** The chain covers the audit rows that were stored. An event that was never published, or that the consumer dead-lettered, is not in the chain, and the chain does not show that it is missing.
- **L39. Audit rows written before a chain was switched on are outside the chain, and they are never rewritten.** A verification report counts them and says that the chained part is verified, not the whole log.
- **L40. A SIEM copy is a witness only for the rows it has received, and only where a customer has configured a destination.** The exporter is off unless `AUDIT_SIEM_EXPORT_ENABLED` is set to `1`. A destination that is disabled, failing or not configured proves nothing about the rows it did not receive.

## WS06-S9
shared-edit docs/compliance/evidence/honest-limits.md (CRLF), part b: insert after the '| L35 |' row of the section 10 basis table, one row per line:
| L37 | The chain is off unless `AUDIT_CHAIN_ENABLED` is `1`; the signing key comes from the audit-core environment, not the database; the guard trigger is a guard rail, not a boundary. | `apps/Control Plane/audit-core/cmd/server/main.go:385 @"chainEnabled := parseChainEnabled"`<br>`apps/Control Plane/audit-core/cmd/server/main.go:298 @"AUDIT_CHAIN_PRIVATE_KEY_BASE64"`<br>`apps/Control Plane/audit-core/internal/store/migrations/007_audit_chain.sql:46 @"Guard rail, not a security boundary"` |
| L38 | A clean verdict covers the rows the verifier read and anchors them to a signed attestation; it does not say the log is complete. | `apps/Control Plane/audit-core/internal/verifier/verifier.go:38 @"the chain from its trust base to the head recomputes"` |
| L39 | Rows before activation are `legacy_unchained`; the chain migration does not backfill them, and the verifier reports them. | `apps/Control Plane/audit-core/internal/store/migrations/007_audit_chain.sql:3 @"nothing is backfilled"`<br>`apps/Control Plane/audit-core/internal/verifier/verifier.go:41 @"and the org also has rows written"` |
| L40 | The exporter is off unless `AUDIT_SIEM_EXPORT_ENABLED` is `1`; the destination routes exist only when `ORG_SIEM_DESTINATIONS_ENABLED` is on; the exporter holds no signing key. | `apps/Control Plane/docker-compose.yml:905 @"AUDIT_SIEM_EXPORT_ENABLED: ${AUDIT_SIEM_EXPORT_ENABLED:-0}"`<br>`apps/Control Plane/audit-core/cmd/siem-exporter/main.go:4 @"holds no ingest credential"`<br>`apps/Control Plane/org-core/internal/http/siem_handlers.go:18 @"ORG_SIEM_DESTINATIONS_ENABLED"` |

## WS06-S9
shared-edit docs/compliance/evidence/trust-claims-pending-reverification.md (CRLF): append at the end of the file, after the last paragraph of section 6:
**Chain status (2026-10-24).** The Control Plane audit chain (hash chain, signed head attestations, verifier, owner and admin compliance read, SIEM exporter) is in the source, inert or switched off by default (WS06 S0 to S8). That is not evidence that the audit trail is chained, verified or exported on any stack. Until the activation evidence for the claimed stack has been read (`apps/Control Plane/docs/AUDIT_CHAIN_RUNBOOK.md`, section 9), no public sentence may say the trail is tamper-evident, chained, verified or exported. The chain's limits are in `honest-limits.md`, L37 to L40. Re-read rows 36 to 41 under steps 1 to 5 above before any of them changes.

## WS06-S9
shared-edit apps/Model Plane/MODEL_PLANE_STATUS.md (CRLF): append this sentence to the end of the paragraph under the '## Enterprise readiness' heading (the paragraph that ends 'remain in Enterprise-next after the MVP gates.'):
 The Control Plane audit chain, verifier, compliance read and SIEM exporter are in the source, inert or switched off by default (WS06 S0 to S8). The operator runbook is `apps/Control Plane/docs/AUDIT_CHAIN_RUNBOOK.md`. Enterprise audit export and retention stay in Enterprise-next until activation evidence and an external witness exist.

## WS08-S1
shared-edit apps/Frontend Plane/verevonv3/playwright.config.ts: line 58, change CHAT_WORKSPACE_SPECS to /chat-(?:workspace|voice-mode|transcript-performance|attachment-gates)\.spec\.ts/ so the spec runs in the 'chat' project against the local stack

## WS08-S1
shared-edit apps/Frontend Plane/verevonv3/src/shared/api/ag-ui-client.ts: in streamAgUi (around line 677) build the body in try/catch the way streamChat now does, and on AttachmentsNotSentError call handlers.onError with the same message shape, or confirm the AG-UI route is unused for attachment turns

## WS02-S9
shared-edit apps/Model Plane/deploy/docker-compose.yml: in the inference-core environment block, beside INFERENCE_CORE_ANTHROPIC_ADAPTIVE_THINKING (~line 942), add: INFERENCE_ANTHROPIC_TIERED_SYSTEM: ${INFERENCE_ANTHROPIC_TIERED_SYSTEM:-false}

