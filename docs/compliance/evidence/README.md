# Compliance evidence: engineering evidence from code

> **Engineering evidence, not legal advice.** Everything here was compiled by reading code and configuration, and nothing was run except a few read-only reads of the developer stack (`docker logs`, `docker inspect`, three `SELECT` statements and one count of variable names through `docker exec`, `git grep`), which are kept in their own files and labelled LIVE where they are used. It describes the repository as it stood on 2026-10-01, it is not reviewed by counsel, and it must not be quoted to a customer, a regulator or on the public trust page as a compliance claim. Where a fact needs a live production environment or a vendor contract it says UNVERIFIED.

This directory sits beside the compliance pack in `docs/compliance/` and does not change it. The pack's status table and its documents are owned elsewhere (counsel's); documents here add evidence and propose, and do not edit the pack. The trust-page edits that J13 proposed were applied to the website sources on 2026-10-03 on the owner's instruction (`claims-vs-code.md` section 7.1), and the drift tests below keep them honest.

## Contents

| File | What it is | How it was produced |
|---|---|---|
| `data-flow-map.md` | What content leaves our control, to whom, with region, retention, ZDR and privacy tier, by route and by data class; the request-path gates declared versus enforced; stale statements; where the pack and the code disagree. | Read from code. |
| `live-reads-2026-10-01.txt` | Boot log lines, three `SELECT` results, a variable count and code searches. | Read-only commands on the developer stack, each named in the file. |
| `posture-snapshot-2026-10-01.txt` | Provider variable names, set or empty, allow-listed values and endpoint hostnames, per container. | `tools/export-provider-posture.sh --known-containers`. |
| `subprocessors-from-code.md` | The vendors and recipients the code can reach, classified, with region and DPA left TBC; the Azure services behind one vendor; the diff against the compliance pack and the trust page. | Read from code with `tools/scan-egress.mjs`, plus one read-only live read. |
| `live-reads-subprocessors-2026-10-01.txt` | For each variable name the vendor catalogue lists as a gate, whether a running container has it set or empty. No values. | Read-only `docker inspect`, cut down to name and state before it is written; the command is at the top of the file. |
| `egress-scan-summary-2026-10-01.txt` | The diffable baseline of the egress scan: one line per vendor, host, variable and dependency, no line numbers. | `tools/scan-egress.mjs --summary`. |
| `proposed/subprocessor-list.diff` | A proposed change to `docs/compliance/subprocessor-list.md`. Not applied: counsel owns that file. | Unified diff, checked with `git apply --check`. |
| `retention-and-erasure-evidence.md` | What is retained and what is deleted, by whom, per thread, per user and per organisation; the erasure matrix and the findings (J12). | Read from code, plus read-only reads of the developer stack. |
| `live-reads-retention-2026-10-01.txt` | The read-only counts and schema reads behind the retention and erasure document. | Read-only `SELECT` statements and log reads, each named in the file. |
| `claims-vs-code.md` | Every public or customer-facing trust claim (the Trust Center, the homepage, the product copy, `VEREVON.md`, the compliance pack) against the code and the stack: verdict, evidence, and since 2026-10-03 what was done to each row (J13). | Read from code and the developer stack. |
| `honest-limits.md` | The customer-tone draft for counsel: what is enforced, what is only declared, what is not offered. | Written from `claims-vs-code.md`. |
| `trust-page-edits.diff` | The proposal of 2026-10-01 for the Trust Center sources. Applied on 2026-10-03, hunk by hunk and with adaptations (`claims-vs-code.md` section 7.1); kept as the record. | Unified diff. |
| `trust-claims-pending-reverification.md` | Every public sentence that mentions ZDR, rerank or contextual retrieval, with what to re-check when the build item behind it lands (a, b, c). | Written on 2026-10-03; its claim ids are the pending entries of `trust-claims.ts`. |
| `live-reads-trust-claims-2026-10-01.txt` | Provider posture of the inference-core container on the evening stack, and a run count. | Posture exporter and read-only reads. |
| `live-reads-trust-claims-2026-10-03.txt` | The refresh of the posture read plus the audit-trail delivery read (queued `tool_action` events against audit-core), dormant-vendor flags and the contextual retrieval flags. | Read-only commands, each named in the file. |
| `live-reads-audit-delivery-2026-10-06.txt` | Why the model plane's audit trail never reached audit-core and what is true now: the stream on the model-plane broker, the stuck outbox rows, what audit-core holds, the broker's refused logins and the credential copies compared for equality, the same failure class in other outbox tables and on the application bus. | Read-only commands (`SELECT`, `docker inspect` and logs, the NATS monitoring endpoint), each named in the file. Postmortem and replay procedure: `apps/Model Plane/docs/postmortem/0002-session-audit-trail-never-delivered.md`. |
| `live-reads-contextual-retrieval-2026-10-06.txt` | The numbers behind the corrected statements about the Data Plane's background AI calls: the contextual retrieval and graph extraction variables (names and allow-listed values, key presence only), the unit, summary, ledger and graph counts, the keyword index against Postgres (and which units it lacks), the cost ledger rows by producer, the inference metrics body, the organisation zero-retention count and (blocks 12 and 13) what the running Auth Core and organisation-service images and the registry hold for the organisation posture. | Read-only commands (`SELECT` aggregates, `docker exec` env filtered by name, Quickwit search counts, one `curl`), each named in the file. Written for the contextual retrieval documents task (`apps/Data Plane v2/docs/contextual-retrieval-runbook.md`). |
| `tools/check-citations.mjs` | The citation checker. | n/a |
| `tools/check-doc-codes.mjs` and `tools/doc-codes.json` | The doc-code checker: every refusal code, claim and switch of the zero-retention documents must be in source (listed per code in the registry) or be named in the registry as designed, and a document that names a designed code must say "not built". | n/a |
| `tools/export-provider-posture.sh` | The posture exporter. | n/a |
| `tools/scan-egress.mjs` | The egress scanner and its vendor catalogue. | n/a |
| `tools/*.test.mjs` | Tests for the tools. | n/a |

Later evidence documents (sub-processors from code, retention and erasure, honest limits, the audit-readiness packet) belong in this directory too and follow the same rules.

## Citations

A claim that rests on code carries a citation in a backtick span. The syntax, shown here in a fenced block so the checker skips it:

```
`<repo-relative path>:<lines>`
`<repo-relative path>:<lines> @"<anchor>"`

lines   one line, a range, or a comma list:   12   12-40   12,30,44-50
anchor  a short string (80 characters at most, no double quote) that must appear
        inside the cited lines
```

Rules:

- The path is relative to the repository root, with forward slashes. Windows paths, absolute paths, `..`, and anything under `.claude/`, `node_modules/`, `target/` or `.git/` are refused: a copied worktree is not the code of record.
- Lines are 1-based and inclusive. A CRLF or LF file counts the same. A final newline adds no line.
- The anchor is what catches drift. A line number that is still in range but now points at other code passes a range check and fails an anchor check, and the message says where the text moved. Put an anchor on any citation into a file that other work is editing.
- A pipe inside an anchor must be written `\|` when it sits in a table cell. Avoid it when you can.
- A backtick span that looks like a citation (a file path with a known extension, then a colon) but does not fit the grammar is reported, never ignored. A space after the colon, an en dash in a range and a trailing colon all fail.

Tables with an **Evidence** column get one more rule: every data row must hold at least one valid citation in that column, or the word `UNVERIFIED`. The checker prints how many UNVERIFIED rows each run saw, so unproven rows stay visible.

Every document must say "not legal advice" in its first 20 lines.

## Checking

From the repository root:

```
node docs/compliance/evidence/tools/check-citations.mjs              # every document in this directory
node docs/compliance/evidence/tools/check-citations.mjs --show FILE  # also print each cited excerpt
node docs/compliance/evidence/tools/check-citations.mjs --json FILE  # machine-readable result
node docs/compliance/evidence/tools/check-doc-codes.mjs              # the zero-retention documents' codes against source (warns when a designed code lands)
node --test docs/compliance/evidence/tools/*.test.mjs               # the tools' own tests
```

Exit status is 0 when clean, 1 for any failure, 2 for a usage error. It fails on: a missing file, a path that is not a regular file, a line past the end of the file or a range running backwards, an anchor that is not in the cited lines, an anchor that is too long or empty, a malformed citation, a forbidden path, a cited file containing NUL bytes (an abrupt shutdown can leave a working-tree file zero-filled), a table row with no evidence, and a missing header.

The second-reader pass is `--show`: re-open every cited excerpt and read the claim against it. A green run proves the citations point at real lines. It does not prove the lines say what the claim says. Only the second read does that.

## The posture exporter

`tools/export-provider-posture.sh` reads the environment of running containers (`docker inspect`, no exec) or of an env file, and prints, per source:

- the NAME of every provider-related variable and whether it is set or empty;
- the value only for an allow-list, and only when the value has the shape that variable should have;
- for endpoint variables, the hostname only: scheme, credentials, port, path and query are dropped.

It never prints an API key, a token, a password, a connection string or a full URL.

Allow-list:

- Core: `*_REGION`, `*_DEPLOYMENT_TYPE`, `*_RESIDENCY`, `MODEL_PLANE_ALLOW_*` (a flag must be a boolean).
- Hostnames only: `*_ENDPOINT`, `*_API_BASE`, `*_BASE_URL`, `COHERE_URL`. A value with no `http`, `https`, `ws` or `wss` scheme is withheld, so a secret pasted into an endpoint variable is not shown as a hostname.
- **Extension to the original spec**, because they decide which provider serves which data and hold no secret: `INFERENCE_PROVIDER_ORDER`, `EMBEDDING_PROVIDER`, `MODEL_PLANE_EMBEDDING_PROVIDER`, `GENERATION_MODEL`, `FAST_MODEL`, `EMBEDDING_MODEL`, `RERANKER_MODEL`, `AGENT_MEMORY_GENERATION_MODEL`, `AGENT_MEMORY_EMBEDDING_MODEL`, `DREAMING_LLM_EXTRACTION`, `DREAMING_LLM_MODEL`, `MEMORY_GROUNDING_ENABLED`, `CODEX_SUBSCRIPTION_TRANSPORT`, `CODEX_APP_SERVER_HARDENED`, `AGENTIC_SUBSCRIPTION_ENABLED`, `AGENTIC_SUBSCRIPTION_ALLOW_EFFECTS`, `QUARRY_EDGE__ZERO_SAAS_SEARCH`, `NOTIFICATION_DELIVERY_MODE`, `VEREVON_INTENT_ENABLED`, `DEFAULT_PROVIDER`. The list is the `selector_names` array in the script; remove it there if the owner wants the original four patterns only.

Shape checks are a second line of defence for the day someone puts a credential in a variable that is allow-listed by name: a region, deployment type or residency label must be letters first, at most 24 characters, at most three digits; a selector must have no long unbroken alphanumeric run and few digits. A failing value prints `[withheld: unexpected shape]`. These checks are heuristics. A short value that happens to look like a region would be shown, so the allow-listed names must never hold secrets. The default container list is `KNOWN_CONTAINERS` in the script and `--container NAME` overrides it.

```
bash docs/compliance/evidence/tools/export-provider-posture.sh --known-containers
bash docs/compliance/evidence/tools/export-provider-posture.sh --container model-plane-inference-core-1
bash docs/compliance/evidence/tools/export-provider-posture.sh --env-file path/to/.env
```

The tests feed it a fixture full of canary secrets and assert that none of them, and no fragment of an endpoint's credentials, appears on stdout or stderr. The script is LF-only on purpose, because a Linux shell chokes on CR.

## The egress scanner

`tools/scan-egress.mjs` finds every outbound host the code can address (URL literals in non-test, non-doc, non-generated source and in compose and env files), the variables that gate each vendor, and the SDK dependencies that hide a host. It sorts them into a vendor catalogue (the `VENDORS` array in the script: class, role, hosts, gating variables) and prints the result sorted, with relative paths and no timestamps, so two runs on one tree are identical. It never writes a region or a DPA status; those stay TBC. It prints a variable's name and whether a compose or env file gives it an empty or non-empty default, never a value.

```
node docs/compliance/evidence/tools/scan-egress.mjs                    # the full report, file and line evidence
node docs/compliance/evidence/tools/scan-egress.mjs --summary          # the diffable form (no line numbers)
node docs/compliance/evidence/tools/scan-egress.mjs --json             # machine-readable (a trust-page drift test reads this)
node docs/compliance/evidence/tools/scan-egress.mjs --strict           # exit 1 on an unclassified host, a stale catalogue entry,
                                                                       # a curated variable found nowhere, or an unmapped credential name
node docs/compliance/evidence/tools/scan-egress.mjs --verify-doc docs/compliance/evidence/subprocessors-from-code.md
```

`--verify-doc` fails when the register in `subprocessors-from-code.md` lacks a row for a vendor the scan found, or when a row marked `EXCLUDED` gives no reason. A new vendor therefore cannot arrive without a decision. The tool's limits are printed at the top of its source: a URL literal shows the code can address a host, not that it is called; hosts built at run time are invisible and only counted.

## The Trust Center drift tests

The public wording is pinned to this evidence by tests that live with the website, in `apps/Frontend Plane/verevonv3/apps/verevon-web/src/components/trust/`:

- `trust-claims.ts` is the machine-readable list of the evidence-backed claims: each entry names the rows of `claims-vs-code.md` it answers, the sentences that must stay on a named surface, the phrases no page may contain, the code anchors the claim rests on, and, when unbuilt work will change it, the build item (a, b or c).
- `trust-data.test.ts` fails when a narrowed sentence is reworded away, a contradicted phrase comes back, an evidence anchor disappears from the code, a pending claim is missing from `trust-claims-pending-reverification.md`, a control or row is upgraded to "live" without a reason, or the sub-processor table stops accounting for every vendor the egress scan finds and every vendor `subprocessors-from-code.md` proposes to list. It runs `tools/scan-egress.mjs`, so it needs `git` and `node` on the path and the whole repository; set `TRUST_REPO_ROOT` to point at a checkout from elsewhere.
- `TrustCenter.test.tsx` renders the page and checks that every control, row and certification reaches it, and that the disclosures are in the page text.
- `trust-recipients.ts` records, with the register's reason, every scanned vendor that has no row.
- In the SPA, `src/features/settings/components/in-product-trust-copy.test.ts`, `src/shared/api/privacy-tier.tooltips.test.ts` and `src/shared/api/privacy-client.test.ts` pin the in-product copy (erase dialog, disconnect note, tier tooltips, export notice) to the code.

```
pnpm --dir "apps/Frontend Plane/verevonv3" exec vitest run apps/verevon-web/src/components/trust
pnpm --dir "apps/Frontend Plane/verevonv3" exec vitest run --config apps/verevon-web/vitest.config.mts
pnpm --dir "apps/Frontend Plane/verevonv3" exec vitest run src/shared/api/privacy-tier src/shared/api/privacy-client src/features/settings/components/in-product-trust-copy
```

A failing anchor is a signal, not a nuisance: the code a sentence rests on changed. Re-read the code, then change the sentence or the anchor, never only the test.

## Limits

- A snapshot or live read describes one machine on one day. It is not production.
- A vendor's retention, regions and sub-processors are contract facts. The code shows what we send and what we check, not what the vendor does.
- Several comments in the code base are stale. `data-flow-map.md` section 5 lists the ones found; do not quote a comment as evidence.
- Nothing here is a statement that a control is "in place". Where the code only declares, the map says DECLARED.
