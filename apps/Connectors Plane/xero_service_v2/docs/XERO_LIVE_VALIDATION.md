# Xero Accounting GET live validation

Validated on 2026-10-02 against the pinned Accounting OpenAPI catalog
(`fd9d44b04bf4934a7509b8e7ece51a9e0e462e4f`). The PKCE connection had
seven organisations. Calls were read-only and used `sync-accounting-get
--dry-run`; no GCS or BigQuery writes were performed. Local response artifacts
are under ignored, private `out/pkce-validation-2/` and must not be committed.

| Measure | Result |
|---|---:|
| Catalogued Accounting GET operations | 126 |
| Distinct operations returning HTTP 200 | 111 |
| Distinct operations attempted but returning 401/404 | 5 |
| Operations not attempted for lack of suitable source data | 10 |
| Direct GET attempts, including repeats and cross-organisation probes | 216 |
| Successful raw response artifacts replayed through the staging loader | 203 |
| JSON / binary artifacts | 183 / 20 |

Every successful artifact passed the local `GCSReader` sidecar/path checks,
Accounting GET dataset routing, and `accounting_get.run` with an in-memory
writer. All 20 binary responses produced pointer-only warehouse rows; no
binary bytes entered the BigQuery payload field. This verifies serialization
and loader compatibility, **not** live GCS IAM, a BigQuery insert, or a full
tenant extraction. The main organisation supplied most route samples; ten
candidate collections were also checked in each of the six other connected
organisations to find IDs and attachment-bearing records.

## Responses needing follow-up

| Operations | Live result | Follow-up |
|---|---|---|
| `getPaymentServices`, `getBrandingThemePaymentServices` | HTTP 401 on old consent | Historical result. These payment-method configuration routes are now deliberately out of warehouse scope; v2 does not request the write-capable `paymentservices` scope or schedule/retest them, and strips the embedded Branding Theme collection before BigQuery staging. |
| `getReportTenNinetyNine` | HTTP 404 | None of the seven connected organisations is US-based (two GB, three AU, one NZ, one CA). `ViewTenNinetyNineReport` is `NOT-ALLOWED` in Organisation Actions for all seven, so this is an unavailable capability in the connected set. |
| `getContactCISSettings`, `getOrganisationCISSettings` | HTTP 404 | Both GB organisations returned 404 for sampled settings; neither returned CIS account types. Xero documents a 404 for contacts never enabled as CIS subcontractors. Do not treat this as proof of a broken route. |

The four generated PDF routes initially returned 404 because the catalog
generator used each OpenAPI `/pdf` path instead of its `x-path` override.
The generator now uses the effective path and sends `Accept: application/pdf`.
All four returned HTTP 200 on a live retry; their bytes begin with `%PDF` and
passed the loader's private-document routing. BFF onboarding also previously
ignored a malformed `.env.bff` scope line, silently reverting to default
scopes. The scope value is now quoted, and a present but invalid `.env.bff`
fails startup instead of weakening the requested consent.
Organisation Actions reported both Payment Services view actions as `ALLOWED`
for all seven, which narrowed the historical 401s to the missing OAuth scope.

References: the pinned [Accounting OpenAPI specification](https://raw.githubusercontent.com/XeroAPI/Xero-OpenAPI/fd9d44b04bf4934a7509b8e7ece51a9e0e462e4f/xero_accounting.yaml)
defines the PDF `x-path` overrides and the `paymentservices` scope;
Xero's [Contacts API guidance](https://developer.xero.com/documentation/api/accounting/contacts)
documents the CIS-contact 404 behavior, and its
[Organisation API guidance](https://developer.xero.com/documentation/api/accounting/organisation/)
describes Organisation Actions and CIS settings.

The remaining ten routes need suitable source records: attachment downloads
by ID/filename for Accounts, Bank Transfers, and Quotes (six routes); Expense
Claim and Prepayment detail and history (four routes). Account and Bank
Transfer collection records explicitly reported `HasAttachments=false` in the
seven connected organisations. Quote list records omit `HasAttachments`, so
their completed pagination does **not** establish that no Quote has an
attachment. Bounded, read-only discovery checked one Quote in one
organisation and all 73 Quotes in another; every attachment list was empty.
The discovery command now supports
single-request Accounts and Bank Transfers scans and requires an explicit
`--include-unflagged` for Quotes; a broader Quote metadata scan is still
needed before ruling out attachment-bearing Quotes. `getReportFromId` was
verified using an ID from another organisation's `Reports` list. Other attachment
families were verified through metadata list, by-ID download, and by-filename
download using attachment-bearing records. Invoice attachment downloads by
both selectors returned byte-identical PDF data.

Classic [Expense Claims](https://developer.xero.com/documentation/api/accounting/expenseclaims/)
are a deprecated Xero API, so they should be validated
only against a tenant with existing historical claims, not by creating new
production fixtures. A Prepayment detail/history fixture requires a genuine
`RECEIVE-PREPAYMENT` or `SPEND-PREPAYMENT` bank transaction, per Xero's
[types and codes](https://developer.xero.com/documentation/api/accounting/types/),
in an authorised
test organisation. No financial records were created during this validation.

## Current entitlement and source-data check: 2026-10-18

The current PKCE connection was tested again against the 150-operation pinned
catalog. The calls were read-only. Forty-eight parameterized Accounting JSON
detail, history, and attachment-metadata responses returned successfully and
were written to the active raw generation. The GCS-to-BigQuery delivery queue
was draining those objects during the check; source success and a durable raw
sidecar/manifest are the evidence for the API result, while the receipt view is
the evidence for eventual warehouse delivery.

| Category | Result | Warehouse interpretation |
|---|---|---|
| `getContactCISSettings`, `getOrganisationCISSettings` | HTTP 404 | CIS is unavailable for the connected organisations. Keep capability status `unavailable`, not a sync failure. |
| `getOnlineInvoice` | HTTP 404 | No online-invoice feature/source record was available for the selected invoice. Treat as feature/data dependent. |
| `getReportTenNinetyNine` | HTTP 404 | 1099 reporting is unavailable for the connected organisations; it is not a Norwegian/AU/NZ/CA warehouse gap. |
| Expense Claim, Prepayment, Receipt detail/history routes | No suitable source record | No parent IDs exist in current staging. Do not manufacture financial fixtures or mark a missing row deleted. |
| `getReportFromId` | No discoverable report ID | The report list did not expose a usable report identifier in this source set. |
| Bank Feeds collections | HTTP 401 for all seven tenants | Requires Xero-approved `bankfeeds` access. Detail routes remain blocked until a collection can return IDs. |
| Finance, including BankStatementsPlus with a real bank-account ID | HTTP 401 | Requires the Finance partner product/entitlement and the documented Finance scopes; re-authorize every PKCE tenant after Xero enables it. |
| Identity `/Connections` | HTTP 200 | API access works. Persisting it requires the separate restricted raw bucket and dataset, which are intentionally not configured. |
| Binary PDFs and attachment downloads | Not warehouse-tested in this environment | `XERO_DOCUMENT_GCS_BUCKET` and its private loader/dataset are absent. Do not send binary documents to the ordinary raw bucket. |
| Payment Services (2 routes) | Not tested | Explicitly outside the warehouse contract. Customer and supplier Payments remain in scope. |

This is not a claim that all 150 operations can return business rows in these
seven organisations. Xero's documented route set includes regional products,
partner products, optional features, and operations requiring a parent object.
The coverage report must distinguish `response_verified`, `unavailable`,
`no_source_record`, and `out_of_scope`; only the first means source data was
successfully obtained.

## Extraction limits

These checks execute one explicitly parameterized request per call. They do
not prove complete pagination, date-window coverage, parent fan-out, or
reconciliation. For example, `getJournals` returned one 100-record page and
an explicit `offset=100` request returned the next page. The direct command
does not issue that second request automatically. The 55 operations outside
existing sync commands still need bounded discovery/checkpointing before they
can be considered an automated warehouse feed.

The first live pilot exposed a GCS key mismatch: object-key normalization
lowercased operation IDs while sidecars retained camel case. The CLI now
uses a lowercase endpoint slug, and the loader maps that slug back to the
catalogued operation ID. Artifacts from the earlier `out/pkce-validation/`
pilot predate this correction and must not be uploaded.

Before production use: test the region/feature-specific routes in suitable
organisations, validate the ten
data-dependent routes in a suitable test tenant, add
bounded fan-out and pagination, run an end-to-end private-bucket/BigQuery
smoke test, and reconcile raw/staging counts per organisation and operation.

## Warehouse verification: 2026-10-06

Read-only verification covered `prj-dw-dev.dw_staging_xero`, the ordinary raw
bucket `prj-dw-dev-raw`, and existing Xero ODS/Silver models in
`europe-north2`. BigQuery queries disabled the query cache and used a 1 GiB
per-query billing limit. Source records and identifiers were compared
privately; only aggregate evidence is reported here. No Xero records, GCP
configuration, warehouse rows, or running services were changed.

The previous MCP overview counted one authorized tenant. This verification
covered all seven warehouse tenants: **461,259 rows in 45 staging tables**
at the observation time. These are different populations, not a growth
comparison. The count-only MCP overview cannot perform these deeper checks;
this round used read-only BigQuery/GCS clients and Cloud Logging metadata.

| Gate | Evidence | Result |
| --- | --- | --- |
| Recent raw-to-staging delivery | Latest incremental run started at 12:00:04 UTC and finished at 12:00:37 UTC (14:00 Oslo). All 985 extracted records matched both sidecar counts and distinct staging keys; all matched staging rows were observed during that run. | Pass for this run's extracted header records. |
| All endpoint availability | Current official OpenAPI operation-ID sets match the local catalog: Accounting 126, Assets 4, Projects 7, Bank Feeds 4, Finance 8, Identity 1. | Pass for the 150 GET operation IDs; not a parameter/schema or warehouse certification. |
| Scheduled endpoint coverage | Today's manifests monitored 28 Accounting collection operations and six reports. 33 operations succeeded; the two Payment Services routes returned 401 for all seven tenants. | Historical partial result; those routes are now out of scope. Exhaustive pagination, detail/history/document fan-out, and the other API families remain uncertified. |
| Downstream freshness | All 22 compared Silver header models had staging keys absent from Silver; nonempty models retained May/June source observations. Six sampled ODS fact tables in L0/L1 retained July 24 source observations. | Fail for end-to-end warehouse freshness. |
| Child-row reconciliation | Two invoices and three purchase orders retained eight extra child rows. Matching raw responses confirmed the smaller header line counts. | Fail. |
| Manual-journal key integrity | 5,653 of 5,712 lines lacked `line_position`; the 59 keyed rows had no duplicate valid keys. | Fail for the required line grain. |
| Durable warehouse run evidence | `sync_runs` and `sync_run_entities` were absent. The bucket's sole notification filters `raw/xero/`, excluding `_manifests/xero/`. | Fail; raw manifests exist but are not automatically loaded. |

The latest incremental run was
`40a21034-3b0d-43ad-b255-5f2adddbd585`. It reported 189 successful and seven
failed tenant/endpoint outcomes, with 985 records across 19 nonempty
collections. All 69 nonempty tenant/endpoint outcomes were compared from
manifest to sidecar to raw response to staging identity. This establishes
delivery of those extracted records, not that the source extraction included
every eligible record. Zero incremental records mean no records in that
request scope, not proof that an organisation has no historical data.

### Reconciliation observations

- Parent/child count checks matched for bank transactions, credit notes,
  journals, manual journals, overpayments, quotes, invoice payments, contact
  addresses, and tracking options. None of the 12 tested relationships had
  orphan children. Count parity alone does not establish field correctness.
- Invoice children totalled 23,766 against a header expectation of 23,762;
  purchase-order children totalled 2,394 against 2,390. Raw source checks for
  all five affected parents confirmed the obsolete extra rows. These historical
  findings are not a request to replace or delete them. The fresh baseline
  preserves observations and marks removal from a complete parent array.
- Manual-journal counts matched despite missing line-position keys. Treat
  the resulting 4,097 excess rows under incomplete keys as a key-integrity
  failure, not proof that 4,097 business lines should be deleted. Do not backfill
  these legacy rows; fresh source responses retain each original line position.
- All 75,203 journals with lines balanced on net amounts within 0.02; 3,045
  headers explicitly expected zero lines. Gross/tax journal sums were not
  used as a double-entry balance gate. Header/line monetary checks also
  flagged additional rounding/amount exceptions requiring classification;
  these are not certified ingestion defects without source comparison.
- Contact phone counts intentionally differ because the parser omits blank
  phone placeholders. The 347 stored phones must not be compared directly
  with the headers' 7,560 raw phone-array entries as a loss assertion.

Cloud Logging showed 54 HTTP 500 requests with no available instance and
one BigQuery concurrent-update exception on October 6 at the observation
time. These indicate capacity/retry risks; they are request attempts, not
proof of 55 lost source objects. The selected latest run's header delivery
passed despite these events. The function was active and configured for
`dw_staging_xero`.

### Fresh-baseline decision (October 6)

1. Start from **new Xero responses only** in an isolated v2 raw namespace and
   `dw_staging_xero.v2_` tables. Do not replay old payloads/manifests or copy
   historical tables into this baseline. Fresh namespace notifications include
   manifests; use the new coverage and stage-only quality views.
2. Preserve every observed version and raw child position. Expose superseded,
   explicit source-deleted, and removed-from-complete-parent markers; do not
   replace/delete stored business rows or backfill legacy journal keys.
3. Stop at staging. Existing downstream gaps remain historical findings, not
   a request to rebuild ODS/Silver now. Source and child counts must reconcile
   inside the same fresh generation and extraction scope.
4. Keep Payment Services and Branding Theme Payment Services out of the
   warehouse contract. Implement/run the remaining in-scope operation-specific
   extraction, pagination, parent discovery, and restricted destinations.
   Assign explicit unavailable/no-source-data outcomes where appropriate; do
   not count them as successful warehouse extraction.

Existing subtotal assertions use inner joins and a relative tolerance. They
can pass with entirely missing line sets, orphan lines, NULL amounts, or
stale models. Certification must include these separate checks and compare
aligned extraction scopes and identities, not just assertion success or
equal aggregate counts.

### Implementation validation and incidents

The fresh contract has immutable record/collection/child stores, atomic ingest
receipts, manifest history, lifecycle views, a 150-operation reference catalog,
and stage-only verification gates. It does not certify fresh source extraction
until new responses have passed those gates. See [Operations](OPERATIONS.md).

Payment Services was isolated using the same app/redirect with `openid
offline_access`: authorization reached the Xero login page. Adding only
`paymentservices` returned `invalid_scope`. This historical scope result does
not affect customer Payments, which use `accounting.payments.read` and remain
in scope. Payment Services is now intentionally excluded from the warehouse.

Existing ETL tests were inadvertently run against real GCP during this work.
Read-only job-statistics inspection found six MERGE jobs for legacy
`staging_xero.bank_transactions` with no affected rows, and six for
`staging_xero.bank_transaction_lines` totaling **60 row-update executions**,
not necessarily 60 distinct rows. The disposable `_test_bq_writer` table also
received ten inserts and four updates across eight MERGEs, and was reset by its
existing test code. Exact previous/new business values were not reconciled;
no rollback, deletion, or further legacy repair was attempted. These are not
writes into the fresh `dw_staging_xero.v2_` baseline. Live tests now require an
explicit isolated test destination, and default offline runs deny Google
clients/ADC/API networking before collection.

### Fresh deployment checks

- `xero-v2-fresh-to-staging` is ACTIVE in `europe-north1`, with retries enabled
  and entrypoint `process_fresh_gcs_upload`. The four critical deployed source
  modules match their local contents. Notification configuration 2 includes
  raw sidecars and manifests under the isolated generation prefix.
- The October 6 17:25 UTC old-namespace event probe returned HTTP 200 and was
  skipped before a raw read or staging publication. This is an isolation smoke
  test, not a successful Xero-to-staging business-data test.
- The fresh reference catalog has 150 operations. Fresh record versions,
  child versions, and ingest receipts remain empty; zero quality issues on an
  empty baseline do not certify source completeness or reconciliation.
- The offline ETL suite passed 644 tests. The 492 focused fresh-contract tests
  achieved 97% statement coverage across the six production modules. Rust GCS
  tests passed 52 unit tests and one integration test; CLI tests passed 26.
  The generation wrapper reached 100% coverage, while combined GCS/CLI coverage
  was 77% due to existing CLI paths. Ruff and the security/code review passed
  for the new contract, with no outstanding CRITICAL/HIGH findings.
- Two Dataform compile checks timed out and were excluded from the final
  offline suite. No downstream Dataform certification is claimed or required
  for this staging-only baseline. Actual fresh source delivery remains pending
  private client-secret rotation. Payment Services is intentionally out of
  scope and does not need app-scope access.
