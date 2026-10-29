# Population gate (gate-b): rules

`scripts/verify_population_vs_visma.py` compares, for each tenant, the open customer and supplier
invoices in the warehouse with Visma's own list of open invoices. A watermark sync cannot see an
invoice that stopped being open without its timestamp moving (a payment does not touch
`lastModifiedDateTime`), so this gate compares whole populations, key by key.

Run it for one source and the tenants you name:

```
.venv/bin/python scripts/verify_population_vs_visma.py --source v2 [--tenants id,id] \
    [--date YYYY-MM-DD] [--max-age-hours 26]
```

Without `--tenants` it covers every tenant in `tenant_scan_compare.TENANTS` (the full gate). With
`--tenants` it writes a `_subset` report, which is not a full gate.

## Verdicts and reasons

Each tenant gets one verdict, and every reason it failed is listed (`reasons` in the JSON, `Reasons`
in the report). Reasons are `error`, `stale` and `defect`. `pending` is not a reason to fail.

| Verdict | When |
|---|---|
| ERROR | `error` applies: the gate cannot give a reliable answer (an ERROR guard, no token, a bad response) |
| FAIL | `stale` and/or `defect` apply, and no error |
| PENDING | no reason, but at least one difference is newer than the tenant's last sync |
| PASS | no reason and no difference |

The verdict is the worst applicable one, in the order ERROR, FAIL, PENDING, PASS. A tenant that is
ERROR and stale shows both reasons, so a stale tenant is never hidden behind an error.

Exit code: 0 only when no tenant has a reason (every tenant is PASS or PENDING). Otherwise 1. An
empty run is 1.

## Differences

Each difference is compared with the tenant's last sync for that endpoint: Visma's
`lastModifiedDateTime` against the last successful sync time.

PENDING kinds: the warehouse is behind Visma, and the change is newer than the last sync.

| Kind | Meaning |
|---|---|
| `open_at_visma_pending` | open in Visma, not open in the warehouse, modified after the last sync |
| `closed_at_visma_pending` | open in the warehouse, closed in Visma, modified after the last sync |
| `balance_pending` | open in both, balance off by more than 0.01, modified after the last sync |

DEFECT kinds: the warehouse should have seen the change at its last sync and did not.

| Kind | Meaning |
|---|---|
| `missed_or_stale` | open in Visma, not open in the warehouse, modified at or before the last sync |
| `closed_without_timestamp` | open in the warehouse, closed in Visma, modified at or before the last sync |
| `balance_stale` | open in both, balance off by more than 0.01, modified at or before the last sync |
| `gone_at_visma` | open in the warehouse, 404 in Visma |

`gone_at_visma` is a DEFECT whatever the time of the last sync. A deletion has no timestamp that a
later sync could catch up with, so the warehouse cannot catch up on its own.

A balance off by exactly 0.01 is cent drift. It is counted, and it is never a verdict.

## ERROR guards

A guard that trips gives the tenant reason `error`. The gate does not guess.

- A warehouse-open key that Visma returns Open by key, but that is missing from the open list.
- A by-key response without `status` or without `lastModifiedDateTime`.
- A by-key response that is not an object (for example a 200 with a null body). It is not read as
  a 404.
- A duplicate key in the Visma open list, or in the warehouse open set.
- A null balance, a null key, or a missing required field (`referenceNumber`, `documentType`,
  `balance`, `status`, `lastModifiedDateTime`) in a Visma row.
- A null balance or a null key in the warehouse open set.
- A list page that is not a list, or that holds a row that is not Open.
- No token for the tenant, or a token refresh that fails.

## Freshness: the last sync

The last sync of a tenant and endpoint is `MAX(finished_at)` of the heartbeat table
`aquatiq-datavarehus.dw_staging_visma._sync_heartbeat`, filtered to `endpoint` and to
`status = 'succeeded'`.

The heartbeat is written by `vsv2-bq-stage` (`tooling/src/vsv2_tooling/bigquery/heartbeat.py`).
After each successful staging write of an endpoint, the same process appends one row:

| Column | Value |
|---|---|
| `tenant_id` | the tenant the batch belongs to |
| `endpoint` | the endpoint name, the same as its staging table (`customer_invoices`, `supplier_invoices`) |
| `finished_at` | when the staging write finished, UTC |
| `status` | `succeeded` |
| `rows_written` | distinct records the write delivered; 0 for an empty batch, which is still a sync |

A heartbeat is never allowed to fail the sync. If BigQuery refuses it, the failure is logged and the
gate reports the endpoint stale.

Why a heartbeat and not a write timestamp: the earlier freshness proxies were the bronze `syncedAt`
and the silver `synced_at`. Those move when a row is rewritten, not when a sync runs. A sync that
writes nothing looked stale, and a silver rebuild moved the threshold.

An endpoint is stale when its last sync is older than `--max-age-hours` (default 26 h).

### No heartbeat on record

An endpoint with no heartbeat is stale. That covers a missing table, an empty table, and a source
that writes none. Until a real sync has written rows to `_sync_heartbeat`, every tenant reports
stale.

With no sync on record, every difference that has a timestamp is PENDING, because nothing is
measured against a sync that is not on record. A `gone_at_visma` stays a DEFECT (see above).

### Source v1

`--source v1` reads `prj-dw-dev` read-only. v1 writes no heartbeat, so every v1 tenant is stale, and
its timestamped differences are PENDING. v1 cannot show DEFECTs until it has a heartbeat of its own.

## Requests and pacing

Visma is read with GET only. Token calls (`vsv2 token --scope erp --tenant <id>`) are requests too.

- One limiter (`PACER`) paces every request of a run, across all tenants. Tenants run one after the
  other.
- The next request starts at least 0.3 s after the previous response returned (or failed).
- A retry after 429, 5xx, a timeout or a reset waits 5, 15 or 45 s before the next attempt.
- The run prints and records the number of requests: `GET` and `token`.

Each run makes, per tenant, two list GETs per endpoint (one page of rows, then the empty page), one
by-key GET per endpoint that has open rows, and one GET by key for each warehouse-open invoice that
is not open in Visma. The last part dominates.

## Output

`docs/gates/population_<source>_<date>.md` and `.json`, or `_subset` when `--tenants` is used. The
JSON keeps the fields that `scripts/run_gates.py` reads. Its `requests` and `heartbeat_table` fields
are new.

## Known limits

- The threshold is `finished_at`, as specified. A record that Visma modified during a sync's fetch
  window can be classed as a DEFECT when the sync could not have seen it. The safe bound is the
  sync's start time. Recording `started_at` would close that window. It is not recorded today.
- A heartbeat failure is logged to the stderr of `vsv2-bq-stage`. The Rust BQ writer discards
  stderr on success, so the log does not reach the orchestrator log. The gate still reports the
  endpoint stale, so the failure is visible there.
- Heartbeats are written only in staging mode (`VSV2_BIGQUERY_DATASET_STAGING` set). Typed bronze
  merges write none.
- The heartbeat table is set in `SOURCES` in the gate. If the staging dataset changes, change it
  there too.
- `finished_at` is stamped by the host that runs `vsv2-bq-stage`, and the gate compares it with its
  own clock. Clock skew between those two hosts shifts every endpoint's age by the same amount.

## See also: gate-s, sales orders per company

`scripts/verify_sales_order_counts.py` compares Visma's SalesOrder `totalCount` per company with
the sales orders in `visma_silver.fact_sales_order`. It fails when more orders are missing than
Visma reports as created or changed since the last sync (`modifiedSince`). The list API ignores an
order-type filter, so the check is per company. It exists because silver once kept one order per
number and dropped 967 orders, which the comparison with v1 could not see. `scripts/run_gates.py`
runs it as gate-s.

## See also: gate-e, endpoint counts per company

`scripts/verify_endpoint_counts.py` asks Visma how many records it holds, for every ERP REST
endpoint that reports a total and for all 12 tenants, and compares that with the live rows
(`deleted_at IS NULL`) of `aquatiq-datavarehus.dw_staging_visma.<endpoint>`. It is gate-s for every
endpoint, against the raw landing tables, so a fetch gap in any endpoint shows up whatever silver
or v1 hold. `scripts/run_gates.py` runs it as gate-e.

```
.venv/bin/python scripts/verify_endpoint_counts.py [--tenants id,id] [--endpoints a,b] \
    [--date YYYY-MM-DD]
```

It writes `docs/gates/endpoint_counts_<date>.md` and `.json` (`_subset` with `--tenants` or
`--endpoints`, which is not a full gate). Exit 0 only when no cell is FAIL or ERROR.

Per endpoint and tenant, `missing` is Visma's total minus staging's live rows, and `changed` is
Visma's count of records modified since the last write (asked only when something is missing).

| Verdict | When |
|---|---|
| FAIL | `missing > changed` |
| WARN | staging has more than Visma (deleted in Visma, pending the weekly pass) |
| PASS | otherwise |
| ERROR | Visma's answer cannot be read (an HTTP status, an unknown shape); it fails the gate |

Where nothing can be pending, `changed` is 0 and the cell says so: the endpoint has no modified
filter that works, or the tenant has no write on record (nothing in staging, so any record in
Visma is missing).

How the total is read, from one request of one row (`pageSize=1`; the `expand*` flags only add
fields and are left out):

| Answer | Total |
|---|---|
| finance list (a JSON list) | `metadata.totalCount` of the first row |
| object (Sales Order API, `records` lists, supplier inbox rules) | `totalCount`, or `metadata.totalCount`, at the top |
| empty page | 0 |
| rows and no total | the endpoint is NOT COUNTABLE: nothing is guessed |

`accounts` and `contacts` ignore `pageSize` and answer the whole list, but still carry the total.
General ledger transactions are counted in one request over every period the fetch walks (ledger
1, `fromPeriod=202001`, `toPeriod=203012`; the pilot's 586,405 lines took 6 s): the total counts
lines and staging holds one row per line.

NOT COUNTABLE is listed with its reason in the report. Some need a period (journals, GL balances,
project transactions), some a parent key (budgets, time cards, project tasks, customer locations),
and the rest answer without any total: seven paged lists and nineteen unpaged ones, which return
their whole list for any `pageSize`. A list's length is not taken as a total.

The modified filter is trusted only after Visma has answered 0 to a request dated 2999, once per
endpoint. Several endpoints ignore it and answer their full total (the Sales Order customer list,
countries, warehouses, VAT zones and others); for those the count of "modified since" would equal
the total and excuse any gap, so the tolerance is 0.

The last write is `MAX(finished_at)` of `_sync_heartbeat` for the endpoint where a row exists, and
else the last staging write (`MAX(updated_at)`); each cell names its source. `_sync_heartbeat` did
not exist on 2026-10-12, so the last staging write stood in, and `updated_at` moves when a payload
changes, not when a sync runs.

A full run makes about 900 GETs (913 on 2026-10-12, 14 minutes), all through the shared `PACER`: one
at a time, 0.3 s after each response, retries after 5, 15 and 45 s, one new token after a 401. An
error is reported as its HTTP status only.

The target dataset is set by `scripts/_bq_target.py` (`VSV2_BIGQUERY_PROJECT`,
`VSV2_BIGQUERY_DATASET_STAGING`, the same settings the pipeline uses), so the gate can read a
scratch dataset. `endpoint_counts_2026-10-13_rehearsal_aps.md` is such a
run: one tenant (Aquatiq Aps) against the dataset that the local dress rehearsal wrote
(`scratch_vsv2_rehearsal`, container image, three passes, see `docs/DEPLOYMENT.md` section 4a).
64 endpoints were counted and all 64 are exact, with the heartbeat as the last write. It shows that
the pipeline a container runs delivers what Visma holds. It is not the full gate: that one is the
12 tenants against `dw_staging_visma`.

Known limits:

- `changed` counts records created or modified, so it is larger than the number that can truly be
  missing: a gap smaller than `changed - missing` passes. With staging last written on 2026-10-01
  and 02, that is wide for the pilot (customer documents 303 missing against 674 changed). The
  gate is sharpest right after a sync.
- The comparison is of counts. A missing record offset by an extra one passes, and no field or
  value is compared.
- A FAIL on an endpoint without a modified filter is not proof of a gap: it can be a record created
  since the last write. Classify it by key (a by-key GET and the record's dates) before acting.
- WARN cells are not investigated by the script.

## See also: gate-a, parity with v1

`dataform/tools/parity_gate.py` runs the four comparison tools (`compare_with_v1`, `compare_columns`,
`compare_values`, `compare_bronze_rows`) against v1 (`prj-dw-dev`, SELECT only) and matches every
finding against `dataform/tools/parity_allowlist.json`. A finding no entry explains fails the gate.

**It is red, and it cannot be green on today's data.** Run on 2026-10-13: 579 findings, 40 explained,
**539 not**, in 99 tables (bronze 305 findings in 55 tables, silver 108 in 18, gold 67 in 9, datamart 16
in 3, silver_common 26, silver_excel 17). It has never been green (385 of 459 on its first run). Why it
is not a go-live gate yet:

* v1 and v2 hold feeds of different ages (v2's staging is a snapshot of 2026-10-01/02 with later
  writes on 2026-10-09; v1 syncs on its own schedule), so every record that changed in between is
  a difference. Most of the 305 bronze findings are of this kind: a field equal in 99 % of the
  records both sides hold, or an invoice that was paid in the meantime. Some are representation
  differences of nested fields (`invoiceLines` is equal in 3 % of the invoices both sides hold), which
  have to be looked at one by one.
* v1's feed is wrong where gates b, c and e prove it (stale open invoices, missing ledger lines), so
  "equal to v1" is not the target there.
* The allow-list was written before the logic fixes of 2026-10-13. Their effects now show as findings
  (project P&L history and business unit, department columns, margin columns, the item, location and
  fixed-asset keys, movement lines). Each is documented, with before and after, in
  `dataform_validation.md` ("Fixed 2026-10-13") and `silver_completeness.md` ("Fixed the same day"),
  but has no allow-list entry yet.

What to do: gates b, c, e and s compare with Visma itself and are the ones that decide correctness. Re-run
gate-a after the first live pass, when both feeds are fresh and the age differences disappear, then give
every remaining finding an entry with evidence or fix it. Until then do not read its exit code as a
verdict on v2.
