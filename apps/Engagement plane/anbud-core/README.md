# anbud-core

Tender (anbud) ingestion core for the Triodelab Ingestion Plane. It catches public tenders from Doffin (and, next, TED), turns every notice into a clean, validated, source-traceable record, and makes that record available so the Model Plane can assess and act on it.

> **Status: raw store and day-window sync built; database tests passed on PostgreSQL 16.14 (2026-10-10).** What runs today: the HTTP service (`/health`, `/ready`, `/api/v1/sources`, and the demo routes), config with fail-closed secret handling, a rate-limited Doffin Public API client, `eforms.Sniff` (reads an eForms notice's SDK version, flavour and identifiers), `cmd/doffin-probe` to verify keys and sample the corpus (it takes the sync's budget lock when `ANBUD_DSN` is set), and `cmd/doffin-sync`, a host-side CLI that copies closed Doffin days into a Postgres raw store (migrations 0002 to 0005). Its pure tests pass, and its database tests passed against PostgreSQL 16.14 on 2026-10-10; no other server version has been tried. The live smoke test of the sync ran on 2026-10-10 (V1 to V4, with the owner's go-ahead, against a scratch database): it passed, and it found that Doffin's tie order is not stable on a multi-page day, which the sync now handles by taking the union of several sweeps (decision D-B3). The census of the full history (V5) has not run. The probe was run against live Doffin and TED on 2026-10-09, before it took the budget lock: both subscription keys work, and [docs/SOURCES.md](docs/SOURCES.md) records what the real data looks like. The database role model that makes the append-only triggers a real boundary (`anbud_migrator`, `anbud_app`, `anbud_dba`, `scripts/roles.sql`, `ANBUD_MIGRATE_DSN`; see [docs/ROLES.md](docs/ROLES.md)) is built and tested for this core; its rollout to the plane's compose setup and other cores is not. The erasure mechanism for notice bodies (`anbud_redact_raw_document()`, tombstones, and the runbook [docs/PERSONAL-DATA.md](docs/PERSONAL-DATA.md)) is built and tested too; the legal decision behind it is not made. Since then, the same day: an eForms parser (contract notices of SDK 1.13 and 1.14, result notices of 1.14; 20 of the 22 captured notices), a normaliser, a gate-and-fit mechanism (`internal/match`), an organisation lookup through org-core, and a demo API (`/api/v1/demo/*`) that parses and assesses the stored notices on every request and stores nothing. Opt-in (`ANBUD_TENDER_DOCS`), a company can also upload the tender documents it holds for a notice as a ZIP, and the gate reads them as far as keyword rules go (`internal/ziputil`, `internal/docread`, `internal/docstore`, `internal/tenderdocs`); only the manifest and the quoted sentences are kept, tenant-scoped and expiring. What does **not** exist yet: the validation report, publication to Data Plane v2 (of documents too), a scheduler for the sync, OCR, the Model Plane actions, and any frontend. See [Build order](docs/ARCHITECTURE.md#build-order) and [Demo API](docs/ARCHITECTURE.md#demo-api).

## Add your key and verify it

`.env` already exists in this directory. It is gitignored (mode 0600) and owned by this core, like every core's `.env` in the plane. Paste the Doffin **Public API** key into it:

```
DOFFIN_API_KEY=<your primary key>
DOFFIN_API_KEY_SECONDARY=<your secondary key, optional>
```

Never paste a key into a chat or commit it. (`TED_API_KEY` is not needed: TED serves published notices anonymously.) Then verify the keys (read-only; one search per key, then about 15 searches and 1 download, paced under the per-key limits):

```bash
set -a; source .env; set +a; go run ./cmd/doffin-probe -save-dir testdata/captured
```

The probe never prints the key. If `ANBUD_DSN` is set, the probe also takes the sync's budget lock and exits 7 when a sync holds it. Without `ANBUD_DSN` it takes no lock, so do not run it while a sync is running ([docs/REQUIREMENTS.md](docs/REQUIREMENTS.md#6-verify-the-doffin-key)). Where to get the key and which API it belongs to: [docs/REQUIREMENTS.md](docs/REQUIREMENTS.md#2-getting-the-doffin-key).

## Sync the raw store

`cmd/doffin-sync` copies closed UTC days into the database named by `ANBUD_DSN`, applying the migrations first. It needs `ANBUD_DSN` and `DOFFIN_API_KEY`, and it runs on the host. The design gives it no compose service:

```bash
GOTOOLCHAIN=local go run ./cmd/doffin-sync -from YYYY-MM-DD [-to YYYY-MM-DD] [-recheck N] [-max-downloads N] [-page-size N] [-census] [-source doffin|ted] [-countries SWE,DNK,...]
```

`-source ted` loads TED (anonymous, no key; default buyer countries NOR, SWE, DNK, FIN, ISL; a notice on both Doffin and TED is shown once). `scripts/install-daily-sync.sh` runs both every morning on this Mac (docs/REQUIREMENTS.md section 9.8).

Flags, exit codes, environment and database rules are in [docs/REQUIREMENTS.md section 9](docs/REQUIREMENTS.md#9-raw-store-sync). The design, its invariants and the open owner decisions are in [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md#raw-store-and-day-window-sync). Do not point the sync at a shared database until the open owner decisions O1 and O2 are settled.

## Ownership and boundaries

anbud-core owns:

- the notice source connectors (Doffin Public API, Doffin year CSV for backfill, TED Search; later the orchestration of Quarry-v2 scrape jobs);
- the canonical **procedure -> notice -> lot** model, normalisation and deterministic validation (the parsed, validated tender is published to Data Plane v2, and this core is not its authoritative store: P1 in [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md#decisions));
- the publish gate toward Data Plane v2, and the typed tender operations the Model Plane may call.

anbud-core does **not** own:

- users, organisations, entitlements (Control Plane);
- reasoning, AI validation, approvals (Model Plane): there is no LLM call in this process;
- documents, retrieval, embeddings (Data Plane v2);
- browser execution (Quarry-v2) or UI (Frontend / Application Plane).

Public notices are shared data, not tenant content. Anything tenant-specific (company profile, go/no-go criteria, drafts) carries the organisation and its retention posture (ZDR). ZDR does not apply to the shared corpus itself (P2).

## Quick facts

| | |
|---|---|
| Language | Go 1.25, Fiber v2, pgx v5, zerolog |
| Port | `3138` (unused anywhere else in `apps/` when chosen) |
| State | Postgres database `anbud_core` in `ingestion-postgres`, schema owned by embedded migrations |
| Auth | `X-API-Key` (or `X-Internal-Api-Key`); tenant routes also need `X-Org-ID` |
| Responses | `{ "data": ... }` / `{ "error": { "code", "message", "details"? } }`; `/health` and `/ready` are plain |
| Events | none yet. New NATS subjects are silently inert without a broker ACL grant, so they are designed with the pipeline |

## Layout

```
cmd/api/                 HTTP service
cmd/doffin-probe/        read-only key check + shape discovery
cmd/doffin-sync/         host-side CLI: day-window sync into the raw store
internal/doffinsync/     sync engine: day windows, completeness rule, error policy
internal/rawstore/       Postgres raw store, sync ledger and budget lock
internal/config/         env loading (secrets as secret.Value)
internal/secret/         credential type that cannot be logged
internal/auth/           API-key auth, org context
internal/api/            routes (health, sources, demo, tender documents), error envelope
internal/eforms/         eForms parser: contract notices SDK 1.13/1.14, result notices 1.14 (eformstest/ holds synthetic fixtures)
internal/normalize/      CPV, NUTS, organisation numbers, Oslo days, the deadline that binds
internal/match/          gate (hard fails) and fit (bands, strictness)
internal/orgcore/        organisation lookup through org-core
internal/demo/           the demo's logic: scan, parse, assess, audit
internal/ziputil/        reads a ZIP that somebody else made: size, bomb and path limits, nested archives
internal/docread/        PDF (in a child process), DOCX, HTML and text to text with page or paragraph numbers
internal/docstore/       what an upload keeps (manifest and findings), tenant-scoped with row level security
internal/tenderdocs/     the upload service: reads an archive, finds what names a capability, keeps the findings
internal/db/             pool, embedded migrations, DSN TLS rule
internal/sources/doffin/ rate-limited Public API client
internal/{validate,lifecycle,actions}/   planned pipeline stages (doc.go only)
docs/                    ARCHITECTURE, REQUIREMENTS, SOURCES
testdata/captured/       gitignored raw captures from the probe
```

## Documentation

| Doc | What it covers |
|---|---|
| [docs/README.md](docs/README.md) | Index of the docs, the commands, and the verification status in short. |
| [docs/REQUIREMENTS.md](docs/REQUIREMENTS.md) | Env vars, getting the key, bring-up, health checks, troubleshooting, and the sync CLI (section 9). **Start here.** |
| [docs/PERSONAL-DATA.md](docs/PERSONAL-DATA.md) | Erasing a notice body: the DBA procedure, the tombstones, the rule after a restore, and the open compliance questions; and what uploaded tender documents mean for personal data (section 9). Not legal advice. |
| [docs/ROLES.md](docs/ROLES.md) | The database role model: the owner, application and DBA roles, `scripts/roles.sql`, `ANBUD_MIGRATE_DSN`, the runtime-role check, and the privileges and row level security of the tenant schema. |
| [docs/SOURCES.md](docs/SOURCES.md) | What Doffin and TED actually expose (verified 2026-10-09 and 2026-10-10), identifier rules, and the live smoke test (V1 to V4 run on 2026-10-10; the census V5 not yet run). |
| [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) | Pipeline, identity model, tenancy, the raw store and day-window sync, Model Plane integration, decisions, open owner decisions, build order. |

## Try the demo API

With a database that `doffin-sync` has filled, the plane's API key in `ANBUD_API_KEY`, and the service running:

```bash
curl -s -X POST http://localhost:3138/api/v1/demo/matches -H "X-API-Key: $ANBUD_API_KEY" -H "Content-Type: application/json" \
  -d '{"profile":{"cpv":["72000000"],"keywords":["fagsystem"]},"strictness":"balanced"}'
```

With `ANBUD_TENDER_DOCS=true`, a company's documents for a notice are uploaded as a ZIP and the same request, with `X-Org-ID`, is judged by them ([docs/REQUIREMENTS.md section 12](docs/REQUIREMENTS.md#12-tender-documents-uploads), [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md#tender-documents)):

```bash
curl -s -X POST http://localhost:3138/api/v1/notices/2026-104837/documents -H "X-API-Key: $ANBUD_API_KEY" -H "X-Org-ID: $ORG_ID" \
  -H "Content-Type: application/zip" --data-binary @Konkurransegrunnlag.zip
```

The answer is a funnel (what happened to every notice read), the tenders that fit, best first, each with its gate and fit checks cited to the notice, and "+N if you widen". Routes, fields, limits and known gaps: [docs/REQUIREMENTS.md section 11](docs/REQUIREMENTS.md#11-demo-api) and [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md#demo-api).

## Tests

```bash
make test       # pure tests; the database tests skip
make test-db    # database tests in a throwaway Postgres container; needs Docker; passed on PostgreSQL 16.14 (2026-10-10)
go vet ./... && go test -race -cover ./...
```

### Public notice catalogue

`POST /api/v1/notices/search` accepts `{ cursor?, query?, buyer?, region?, cpv?, status?, deadline?, notice_type?, sort?, direction? }` under existing service-key authentication. It reads the complete live corpus under the service's concurrency limit and scan timeout, computes self-excluding filter facets and `total_matched`, sorts globally, and then returns up to 50 records. A scan that cannot complete fails rather than reporting partial counts. Cursors bind the filter/sort request and the notice revision so later insertions do not move existing pages. Supported sort keys cover every visible column, with `asc`/`desc`, missing values last, and deterministic notice-ID ties. Amount comparisons retain exact decimal precision; duration ordering uses standardized month/year lengths, while displayed durations remain exactly as stated. All reads continue through `raw_documents_live`, excluding superseded and erased bodies.

`go run ./cmd/import-held-notices` can copy an existing live-evidence snapshot from NDJSON on stdin into `ANBUD_DSN`. Each record uses the `doffin.Notice` fields (`ID`, `Body` as base64, `SHA256`, `ContentType`, `FetchedAt`). It preserves evidence, deduplicates through `StoreNotice`, and records an import run without marking any source day complete. Export only live evidence from the source; this is a snapshot transfer, not a replacement for ongoing source synchronization.

`POST /api/v1/notices/:notice_id` reads the latest live public notice without a profile. It returns the same typed catalogue metadata plus source description, document references, lots, qualification criteria, and evidence provenance. Framework/DPS labels are derived only from recognized, namespace-qualified eForms contracting-system codes; absent fields remain unknown. Issuance time is exposed as `issued_at`, not mislabeled as publication time. Source descriptions are bounded to 30,000 runes with an explicit truncation flag. Private customer-uploaded documents and contact-person fields are not part of this catalogue detail contract.


Catalogue metadata also includes publisher dates from held Doffin search evidence, verified winner views following the existing incumbency policy, and each lot's stated duration and contract end (including an explicit basis for calculated dates). Publication dates are never filled from issuance, requested-publication, or fetch dates. Search evidence is read from the latest 1,000 held pages; absent evidence stays absent. The frontend shows counts for notice kinds, deadline status, deadlines, buyers, regions and CPV choices, and exposes sorting on every table header. Winner names remain omitted if the register is unavailable, unverified, or identifies a sole trader.
