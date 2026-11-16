# Space agent self-API (WS18): candidate and live evidence, and ledger rows

Use this template for the WS18 evidence record, the decisions-ledger entry and the capability-promotion rows. It is filled in by the operator after a run of `apps/Model Plane/scripts/tests/space-self-api-proof.sh`. The slice that wrote the proof did not run it, so nothing below is filled in yet.

Rules for every record:

- Store the evidence directory outside the repository (`PROOF_EVIDENCE_DIR`) and attach the record to the PR. Ids, kinds, digests, counts, status codes and SQL rows of ids only. Never a token, key, password, DSN, an `Authorization` header or a cookie. A HAR file is redacted before it is kept. Decoded decision claims go in with the signature segment left out.
- The decisions-ledger entry (`apps/Model Plane/docs/decisions/ledger.md`) is written by L0 only. No slice edits that file (plan 4.4.1, row 24). Use section 6 as the text L0 appends.
- Promotion beyond `source_only` is a separate step, and it needs candidate and live evidence. Section 7 gives the rows and the rules the promotion-ledger test enforces.

## 1. Candidate header

| Field | Value |
|---|---|
| Candidate commit (immutable sha) | |
| Image digests: session-core, execution-core, capability-core, model-gateway, user-core, gateway | |
| Convex deploy: gateway restarted, "Convex functions ready!" seen (yes or no) | |
| Proof run (UTC date and time) | |
| Stack identity (a name, not a host) | |
| Org and room (refs only) | |
| Operator (a role, not a name) | |

## 2. Switch state at capture

Record the name and the value each switch had when the run started. Values are on or off, a version, or the allowlist; none is a secret.

| Switch | Value at capture | Set by |
|---|---|---|
| `MODEL_ACTION_ADMISSION` | | |
| `OWNER_LIST_SPACE_REFUSE` | | |
| `CONTROL_SPACE_SELF_API_ACTIONS` | | |
| `SPACE_SELF_API_SESSION_CORE` | | |
| `EXECUTION_CORE_SPACE_SELF_API` | | |
| `EXECUTION_CORE_SPACE_SELF_API_RESUMABLE` (must be `0` until WS10-S9) | | |
| `MODEL_GATEWAY_SPACE_SELF_API` | | |
| `SPACE_AGENT_SELF_API_API` | | |
| Control decision key id (names match, yes or no; no key value) | | |
| Capability rows, five `cap.tool.space.*` ids: state after attestation | | |
| Principal scopes granted (names only): `session:run-thread-action:probe`, `capability:owner-action:health:write` | | |

## 3. Proof result

- Layers 1 and 2: copy the `PASS` and `BLOCKED` lines the script printed, and the exit status. A `BLOCKED` line is a result, not a detail to tidy away.
- Phases recorded in `phases.txt`: `flow`, `tokens`, `allowlist-empty`, `zdr`. A missing phase means the proof is not complete.
- Files in the evidence directory, by name only.

## 4. Negatives

Each negative needs its own evidence. "Not run" is a valid entry; a missing row is not.

| # | Negative | Expected | Evidence file | Result |
|---|---|---|---|---|
| 1 | Human title set, then the agent asks to rename | Title unchanged; `title_source` `owner`; no applied rename receipt after the human edit | `sql-thread-flow.txt` and the receipts file | |
| 2 | A viewer switches the self-API on | 403 | the flow phase summary | |
| 3 | Switch off, then a mention | No receipt | the flow phase summary | |
| 4 | Forged decision signature | Refused; no receipt | the tokens phase summary | |
| 5 | Changed arguments under the decision token | Refused (digest mismatch); no receipt | the tokens phase summary | |
| 6 | Another action's arguments under a rename token | Refused; no receipt | the tokens phase summary | |
| 7 | Replay: same token, key and arguments | `REPLAYED`; still one receipt | the tokens phase summary | |
| 8 | Expired decision token (optional input) | Refused | the tokens phase summary | |
| 9 | `CONTROL_SPACE_SELF_API_ACTIONS` empty | No receipt | `allowlist-empty` summary and the operator's note of the variable, by name | |
| 10 | Zero-retention room | No tool; no receipt; no agent override; typed refusal in the log, ids only | `zdr-phase.txt` and the zdr summary | |
| 11 | Removed member's run | No decision (operator-run) | operator's note | |
| 12 | The model names another thread | Argument rejected (operator-run) | operator's note | |

## 5. Capture list

- SQL output: the thread row (`presentation_title`, `title_source`, `pinned_at`, `pinned_source`), the receipts (action, outcome, digest length, idempotency key), the events (type, resource ref, count) and a count of event payloads that carry the title text, which must be zero.
- Decoded decision claims with the signature removed (`decision-claims.json`).
- HAR (redacted), and screenshots of the toggle, the agent chip and the denial copy.
- Content-free logs: ids and kinds only.
- Test summaries: the layer 1 and 2 lines, the registry sync output, and the ledger rows below.

## 6. Decisions-ledger entry (L0 appends; the slice does not)

Template, with placeholders in angle brackets:

```
## <YYYY-MM-DD> - WS18 agent self-API on Space actions: <what was proven, and the level reached>

<Scope: run-bound rename and pin on the run owner's thread; post_card only after WS18-S7. Level: source only, or candidate, or live (name the one). Switches that were on for the run, by name. What was not run, and why. Where the evidence is kept, outside the repository. No secrets, no person's name.>
```

## 7. Capability-promotion rows

The promotion ledger is `apps/Model Plane/docs/CAPABILITY_PROMOTION_LEDGER.tsv`. Its header is tab separated: `capability_id`, `state`, `source_revision`, `candidate_ref`, `live_evidence_ref`, `rollback_ref`, `expires_at`, `reviewer`, `notes`. The states the test accepts are `source_only`, `candidate`, `live_proven`, `allowlisted` and `rolled_back`. The rules the test enforces:

- `source_only`: `candidate_ref`, `live_evidence_ref`, `rollback_ref` and `expires_at` are all `-`.
- `candidate`: `candidate_ref` and `expires_at` are set; `live_evidence_ref` and `rollback_ref` are `-`.
- `live_proven` and `allowlisted`: `candidate_ref`, `live_evidence_ref` and `expires_at` are set.
- `allowlisted`: also `rollback_ref` is set.

`rolled_back` is accepted by the test's state list; check the rule for it in `capability-promotion-ledger-test.sh` before using it.

The five WS18 rows are `source_only` at the time of this template: `cap.tool.space.thread.rename`, `cap.tool.space.thread.pin`, `cap.tool.space.thread.post_card`, `cap.tool.space.agent.pause_self` and `cap.tool.space.schedule.follow_up`. `cap.tool.ticket.create` stays `source_only` and is not part of this record. A candidate row for rename looks like this (tabs between the fields; `<...>` is a placeholder):

```
cap.tool.space.thread.rename	candidate	<candidate commit sha>	<candidate ref>	-	-	<expiry, UTC date>	<reviewer role>	<one line: what the candidate proves and what it does not>
```
