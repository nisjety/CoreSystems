# Secret scanning (Phase 3 PR-0a)

Goal: **no new secret ever enters git history.** This complements the existing
`.gitignore` (which already excludes every `.env` variant and known secret
files) and `docs/SECRET_REMEDIATION_RUNBOOK.md` (rotation procedure).

## What enforces it

| Layer | File | Scans | Blocks |
|---|---|---|---|
| CI gate | `.github/workflows/secret-scan.yml` | commits in the push / PR | merge of a PR that adds a secret |
| Local hook (opt-in) | `.githooks/pre-commit` | the staged diff | a local commit that adds a secret |
| Config (shared) | `.gitleaks.toml` | rules + allowlist | — |

Both layers scan **only new changes**, not full history, so they do not fail on
pre-existing tracked findings — they stop the *next* secret.

## Enable the local hook (recommended, once per clone)

```bash
git config core.hooksPath .githooks
```

Uses a local `gitleaks` binary if installed, else the official Docker image; if
neither is present it warns and lets the commit through (CI is the backstop).

## Verified

`gitleaks protect --staged` flags a planted AWS key (`leaks found: 1`, non-zero
exit) and passes on a clean staged diff. The CI job uses `gitleaks-action@v2`
with `GITLEAKS_CONFIG=.gitleaks.toml`.

## Known pre-existing tracked findings (rotate/redact at push time)

Phase 3 recon found ~22 high-entropy values already committed in **tracked**
files (docs, a couple of scripts/tests, and one template). These are *not* the
gitignored `.env` files and are out of scope for the new-commit gate, but they
**will** be pushed as-is until handled. Highest priority — a tracked template
must never hold a real secret:

- `apps/Ingestion Plane/finspo-core/.env.example` — verify line 5 is a placeholder.

Others (review before first push): `velion-gap.md`, `EXTERNAL_SERVICES_*.md`,
`docs/FRONTEND_BACKEND_INTEGRATION_SUMMARY.md`, `docs/EXTERNAL_SERVICES_ARCHITECTURE.md`,
`docs/INTEGRATION_TEST_REPORT.md`, `SEAMLESS_AUTH_DEPLOYMENT.md`,
`apps/Control Plane/auth-core/scripts/add-password-to-user.js`,
`apps/Ingestion Plane/smoke-test-integration-api.sh`,
`apps/Ingestion Plane/README.md`, and the `execution-core` test JWT fixture
(a known fake). Run `gitleaks dir . --config .gitleaks.toml --redact` for the full list.

> Decision (maintainer, 2026-06-21): no history scrub; live keys rotated when the
> repo is first pushed to a remote (there is no remote today). Keep `.gitignore`
> authoritative so secrets are never staged.
