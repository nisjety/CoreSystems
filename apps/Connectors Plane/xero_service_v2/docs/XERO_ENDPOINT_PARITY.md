# Xero OpenAPI endpoint parity

Baseline: XeroAPI/Xero-OpenAPI commit
[`fd9d44b04bf4934a7509b8e7ece51a9e0e462e4f`](https://github.com/XeroAPI/Xero-OpenAPI/commit/fd9d44b04bf4934a7509b8e7ece51a9e0e462e4f)
(checked 2026-10-02). Count distinct method/path operations, not entity types.
This is a source-level inventory, **not** a live-tenant certification. A GET
path is only useful for the warehouse when its fan-out, pagination, storage,
loader, and reconciliation are also verified.

Read the [2026-10-02 live validation](XERO_LIVE_VALIDATION.md) for the current
PKCE test results: 111 distinct Accounting GET operations returned 200 across
seven connected organisations; five returned 401/404, and ten lacked
suitable source records. This does not change the automation limits below.

| API | GET | Other methods | Current read implementation |
|---|---:|---:|---|
| [Accounting](https://raw.githubusercontent.com/XeroAPI/Xero-OpenAPI/fd9d44b04bf4934a7509b8e7ece51a9e0e462e4f/xero_accounting.yaml) | 126 | 109 | All 126 GET operations catalogued; 124 are directly callable with explicit parameters. Payment Services and Branding Theme Payment Services are deliberately out of warehouse scope; 71 other routes are covered by existing sync commands. |
| [Assets](https://raw.githubusercontent.com/XeroAPI/Xero-OpenAPI/fd9d44b04bf4934a7509b8e7ece51a9e0e462e4f/xero_assets.yaml) | 4 | 2 | 4 GET paths in `sync-extended`. |
| [Bank Feeds](https://raw.githubusercontent.com/XeroAPI/Xero-OpenAPI/fd9d44b04bf4934a7509b8e7ece51a9e0e462e4f/xero_bankfeeds.yaml) | 4 | 3 | 4 GET paths in `sync-extended`; entitlement required. |
| [Projects](https://raw.githubusercontent.com/XeroAPI/Xero-OpenAPI/fd9d44b04bf4934a7509b8e7ece51a9e0e462e4f/xero-projects.yaml) | 7 | 9 | 7 GET paths in `sync-extended`. |
| [Identity](https://raw.githubusercontent.com/XeroAPI/Xero-OpenAPI/fd9d44b04bf4934a7509b8e7ece51a9e0e462e4f/xero-identity.yaml) | 1 | 1 | `/connections` in `sync-extended`. |
| [Finance](https://raw.githubusercontent.com/XeroAPI/Xero-OpenAPI/fd9d44b04bf4934a7509b8e7ece51a9e0e462e4f/xero-finance.yaml) | 8 | 0 | 8 GET paths, with explicit BankStatementsPlus account IDs; partner access required. |
| **Total** | **150** | **124** | **Specification inventory is complete; two Payment Services routes are intentionally excluded from warehouse extraction.** |

## Accounting GET coverage

| Path class | Spec paths | Existing sync commands | Direct catalog command | Remaining automation |
|---|---:|---:|---:|---|
| Collections and configuration | 30 | 28 | 29 | Schedule the remaining in-scope configuration route. |
| Resource details and special ID lookups | 32 | 24 | 31 | Discover alternate IDs and preserve both list and detail envelopes. |
| Attachment metadata lists | 11 | 11 | 11 | Expand bounded parent discovery. |
| Attachment binary downloads | 22 | 0 | 22 | Discover IDs/names; verify private-bucket IAM and retention. |
| History | 16 | 0 | 16 | Fan out per resource and model history records. |
| PDF | 4 | 0 | 4 | Decide retention and document access policy. |
| Reports | 11 | 8 | 11 | Schedule missing reports and test region-specific behavior. |
| **Total** | **126** | **71** | **124** | **53 in-scope routes still lack automatic extraction; two are deliberately excluded.** |

`xero sync-accounting-get --operation <OpenAPI-operationId>` uses a catalog
generated from the pinned specification. It validates all declared path,
query, and header parameters, rate limits and retries the call, bounds bodies
to 50 MiB, and lands the raw response plus sidecar and run manifest. The loader
inserts one append-only `accounting_get_responses` row per successful response:
JSON payload text (up to 8 MiB) or a pointer to raw GCS, and only a pointer for
PDFs/attachments in `staging_xero_documents`. The 26 binary routes require a
separate document bucket. This command makes a specified in-scope operation callable;
it does **not** enumerate the IDs, pages, date windows or region/scopes needed
to exhaustively extract a tenant. It is not on the daily scheduler.

The 71 count is **callable path coverage**, not complete extraction coverage:
22 detail routes require `xero sync --expand-details`, reports are opt-in,
attachment lists need a parent ID, and default Accounting sync covers 27
collection entities. Payment Services and Branding Theme Payment Services stay
in the specification catalog but are rejected by the CLI and marked
`out_of_scope` in staging coverage when absent; any observed execution is a
`policy_violation`. The embedded `PaymentServices` array from a Branding Theme
is removed before its v2 BigQuery staging record is written. Detail
expansion currently lands full detail envelopes in place of collection pages
for that entity/run; independent preservation of both list and detail responses
and field-level reconciliation are still needed for warehouse completeness.
The non-Accounting 24 paths are implemented but not yet live-verified
against an entitled tenant or deployed warehouse. The current Silver models
are a subset of raw/staging, not one table per path.

## Completion gates before MCP

1. Decide whether “every endpoint” means read-only warehouse extraction or
   all 274 operations. The remaining 124 write/delete methods change Xero
   state and should not be silently added to a data warehouse sync or MCP.
2. Add bounded fan-out and checkpointing for the 53 in-scope routes outside existing
   sync commands, including parent/alternate-ID discovery, pagination and
   reconciliation. The direct catalog command does not supply these.
3. Run an entitled test tenant across all six families. Reconcile raw page
   counts to staging, collection membership, and typed Silver models. Keep
   restricted Finance/Bank Feeds/Identity data separate.
4. Deploy and exercise the shared PKCE store with a real private GCS bucket:
   connect, rotate, restart, concurrent workers, and recovery from an
   ambiguous refresh response. The mocked GCS tests are not a live validation.

No MCP endpoint should claim complete Xero coverage until these gates pass.
