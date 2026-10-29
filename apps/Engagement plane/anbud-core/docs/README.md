# anbud-core docs

Status (2026-10-10): the raw store and the day-window sync (`cmd/doffin-sync`) are built. Their pure tests pass, and their database tests passed for the first time on 2026-10-10, against PostgreSQL 16.14. The probe (`cmd/doffin-probe`) now takes the sync's budget lock when `ANBUD_DSN` is set. The live smoke test of the sync (V1 to V4) ran against Doffin the same day with the owner's go-ahead: it passed, and it led to the union-of-sweeps rule for multi-page days (D-B3). The census of the full history (V5) has not run. The database role model (O2, [ROLES.md](ROLES.md)) and the erasure mechanism for notice bodies (O1, [PERSONAL-DATA.md](PERSONAL-DATA.md)) are built and tested for this core; the legal decision behind erasure is not made, and neither has been rolled out to a real database. Later the same day the eForms parser (contract notices SDK 1.13 and 1.14, result notices 1.14; 20 of 22 captured notices), the normaliser, the gate and fit (`internal/match`), the org-core organisation lookup and a demo API were built; see [ARCHITECTURE.md, Demo API](ARCHITECTURE.md#demo-api). Opt-in, a company can upload the tender documents it holds for a notice as a ZIP and the gate reads them ([ARCHITECTURE.md, Tender documents](ARCHITECTURE.md#tender-documents)). On 2026-10-12 the parser learnt contract periods, renewals and signing days, and the demo API gained the incumbency panel on the audit sheet and a re-tender radar (`POST /api/v1/demo/retenders`); winners are named only when org-core confirms they are not sole traders. On 2026-10-13 amendments are tracked: a tender's later notices are grouped with it, the list shows the current one and the audit sheet shows its history ([ARCHITECTURE.md, Amendments](ARCHITECTURE.md)). The re-tender radar also takes the same strictness slider as the matches, default balanced; its `widening` says what each wider step would add. The validation report, publication to Data Plane v2, OCR and the Model Plane actions are not built.

| Doc | Read it for |
|---|---|
| [REQUIREMENTS.md](REQUIREMENTS.md) | Environment variables, getting the Doffin key, bring-up, health checks, troubleshooting. Section 9 is the sync CLI: flags, exit codes, environment, database rules, tests. Section 10 is the org-core lookup, section 11 the demo API with curl examples, section 12 the upload of tender documents. **Start here.** |
| [PERSONAL-DATA.md](PERSONAL-DATA.md) | Erasing a notice body (O1): what holds personal data, the DBA procedure with copy-paste SQL, the tombstones, the rule after a restore, rules for readers, and 20 open compliance questions, and what uploaded tender documents mean for personal data (section 9). Not legal advice. |
| [ROLES.md](ROLES.md) | The database role model (O2): `anbud_migrator`, `anbud_app`, `anbud_dba`, `scripts/roles.sql`, the privilege matrix, credentials and the runtime-role check. |
| [SOURCES.md](SOURCES.md) | What Doffin exposes, as observed on 2026-10-09 and 2026-10-10. The live smoke test (V1 to V4 run, the census V5 not yet run) and what it found. |
| [ARCHITECTURE.md](ARCHITECTURE.md) | Pipeline, identity model, tenancy, the raw store and day-window sync (tables, closed days, completeness rule, error policy, budget arithmetic), decisions (P1 to P3 approved 2026-10-10), open owner decisions (O1, O2), build order. |

## Commands

Run from the module root (`apps/Ingestion Plane/anbud-core`).

```bash
make test       # pure tests for the whole module; database tests skip
make test-db    # database tests in a throwaway Postgres container; needs Docker; passed on PostgreSQL 16.14 (2026-10-10)
```

The sync is run with `GOTOOLCHAIN=local go run ./cmd/doffin-sync -from YYYY-MM-DD [-to YYYY-MM-DD] [-recheck N] [-max-downloads N] [-page-size N] [-census]`. Do not point it at a shared database until the open owner decisions O1 and O2 in [ARCHITECTURE.md](ARCHITECTURE.md#open-owner-decisions) are settled.

## Verification

[ARCHITECTURE.md, Verification status](ARCHITECTURE.md#verification-status) states what has run. In short: the pure tests pass; the database tests passed on 2026-10-10 through `make test-db` against PostgreSQL 16.14, most recently with the sweep rule, the role model and the erasure mechanism (no other server version has been tried); and the live smoke test (V1 to V4) ran against Doffin on 2026-10-10. Not verified live: 404 and 429 handling, saturated days, refused bodies, any day over 100 hits, and the census (V5).
