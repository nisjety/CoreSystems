# D2-A-openapi-null progress (2026-10-28)

Unit: decision 3.10 OPENAPI-NULL (accepted 2026-10-28, option B). Branch `quarryv2-next`, local commits only.

## Committed

| Commit | What | Check |
|---|---|---|
| 6d9eb115d | docs/openapi.yaml: ProfileCookieView.value `type: [string, 'null']`; sdks/typescript/docs regenerated (25 changed, 41 new pages); new guard test scripts/tests/openapi-no-bare-null.test.mjs | see below |
| (next) | notes: "13 places" corrected in depth/D-D-B3-openapi-sdk.md and D-D-B3-openapi-sdk-progress.md; this file; depth/D2-A-openapi-null.md | docs only |

## Checks run

- `MSYS_NO_PATHCONV=1 sdks/generate.sh` under with-lock.sh docker: rc=0 (Python and TypeScript). Log: scratchpad/D2-A-openapi-null/gen.log.
- `tsc -p sdks/typescript --noEmit`, TypeScript 5.9.3 (installed in scratchpad/D2-A-openapi-null/ts): rc=0.
- Python SDK tests: 28 passed (pytest 8, pydantic >= 2.11, deps from requirements.txt installed with `pip --target` into scratchpad/D2-A-openapi-null/pylib).
- `node --test scripts/tests/*.test.mjs` in Quarry-v2: 44 tests, 32 pass, 0 fail, 12 todo, rc=0. Repo root: 143 pass, 0 fail, rc=0.
- After the commit `git status` is clean under sdks/ and docs/ (the spa-verify drift check compares exactly this).

## Traps

- `python -m venv` fails on this host (ensurepip exits 1 under the scratchpad path); use `pip install --target <dir>` and `PYTHONPATH=".;<dir>"`.
- The Python smoke tests need `PYTHONDONTWRITEBYTECODE=1 -p no:cacheprovider` to leave no stray files in sdks/python (gitignored anyway).
- Generated sdks/typescript/src (gitignored) was regenerated and compiles; it is not committed.
