# retrieval-eval-py Research Dive

Generated: 2026-06-07
Updated: 2026-07-10 (live re-verification pass; supersedes the 2026-06-07 snapshot below where it conflicts)

Scope: `apps/Data Plane v2/services/retrieval-eval-py`

## Secure-MVP current state — 2026-07-10

- **Implemented/tested:** nothing. This path has no tracked source, manifest, test,
  or runtime contract. It did not participate in the security program.
- **Built/deployed/reachable/effective:** none. `data-quality-go` remains the active
  eval/quality implementation.
- **Coverage:** not applicable because no code exists; this is not evidence that
  Data Plane eval behavior meets any coverage or quality gate.
- **MVP gate:** do not represent this directory as a service or rely on it for
  release acceptance. Creating a Python eval lab is separate future work.

The remainder is historical repository-orientation detail, not runtime evidence.

## Historical repository verification (superseded for current state)

**This is dead scaffolding, not live code.** `services/retrieval-eval-py/` is a completely empty directory on disk — no source files, no package manifest, no README, not even a `.gitkeep`. It has never contained a single tracked file in this repository's history. `apps/STALE_DOC_DELETION_REGISTER.md`'s claim is re-confirmed: `data-quality-go` (Go, `services/data-quality-go/`, container `dpv2-data-quality`, port `8013`) is the sole active retrieval-eval/quality surface in Data Plane v2. Nothing changed here since the 2026-06-07 snapshot below — the directory was empty then and remains empty now.

| Claim | Verdict | Evidence |
|---|---|---|
| "The directory exists, but the tree is effectively empty" | **Confirmed, stronger than stated** | `find services/retrieval-eval-py -type f \| wc -l` → `0`. `ls -la` shows only `.` and `..` (created `May 7 17:14:56 2026`, never modified since). |
| "no `main.py`", "no package manifest" | Confirmed | No files of any kind exist to check — there is nothing to enumerate. |
| Directory is at least tracked in git as a placeholder | **Contradicted** | `git ls-files services/retrieval-eval-py` returns nothing, and `git log --oneline -- services/retrieval-eval-py` returns nothing. Git does not track empty directories, so this path has **no git history at all** — it is a local filesystem artifact only, not a checked-in scaffold marker. Any teammate who clones fresh will not see this directory until someone puts a file in it. |
| `data-quality-go` is the active eval surface | Confirmed | `docs/core-research/data-quality-go.md` (updated 2026-07-10) shows `dpv2-data-quality` `Up 14 hours (healthy)` via live `docker ps`, with `/health`, `/v1/quality/gates`, `/v1/cost/summary`, `/v1/evals/retrieval` all responding. `docker-compose.yml` defines no service or container for `retrieval-eval-py` anywhere in the file. |
| Other docs' framing of `retrieval-eval-py` | Mixed — see below | Several docs (`docs/gap-data.md`, `DATA_PLANE_DEEP_DIVE.md`, `docs/WIRE_SURFACE_PLAN.md`, `docs/core-research/plane-audit-2026-07-10.md`) already call it "scaffold", "placeholder", "offline lab", or "not present" — consistent with the empty-directory finding. None currently claim it has runtime code. |

## Runtime Shape

No runtime entrypoint exists, because no files exist:

- no `main.py` or any `.py` file
- no `pyproject.toml`, `requirements.txt`, `setup.py`, or any other package manifest
- no `Dockerfile`
- no `README`
- not referenced anywhere in `docker-compose.yml` (checked full-file `grep`)
- not referenced in the `Makefile`'s `make test-integration` / `make check-rs` / `make build-go` targets

## Relationship Read

Current relationship state, re-verified 2026-07-10:

- docs -> `retrieval-eval-py`
  - `docs/gap-data.md` lists it as `SCAFFOLD` / `(placeholder)` and as a `DIVERGED ACCEPTED` line: the RAGAS/DeepEval-style Python harness it was meant to become is explicitly not on the release-gate critical path — `data-quality-go/internal/eval` covers recall@10/nDCG@10/MRR/latency-p95 instead.
  - `DATA_PLANE_DEEP_DIVE.md` lists it as "docs reference only... currently not present as a populated runtime service" and flags the doc/reality mismatch as an open cleanup item (its own item 4 under "next decisions").
  - `docs/WIRE_SURFACE_PLAN.md` lists it as an "offline lab" with no HTTP/gRPC/NATS wiring (all three columns `❌`).
  - `docs/core-research/plane-audit-2026-07-10.md` re-confirms the register's claim as of 2026-07-10 in its own findings list.
- runtime -> `data-quality-go`
  - the actual, only active eval/quality surface: eval runs, trust scoring, quality gates, lint, cost summary, all live at `:8013`.
  - carries its own caveats (see `docs/core-research/data-quality-go.md`): `GET /v1/evals/retrieval/{evalID}` is a hardcoded stub that discards computed scorecards, `GET /v1/quality/lint` 500s on a `wiki_pages.deleted_at` schema-drift bug, and the eval scoring formulas themselves are a synthetic shape-of-response proxy rather than relevance-judged IR metrics — none of that is a `retrieval-eval-py` problem, but it means "the active surface is real" should not be read as "the active surface's numbers are rigorous."

## Cleanup Read

Do not delete the directory blindly unless upstream tooling confirms it is unused — but note there is effectively nothing to delete (an empty, untracked directory carries no history risk either way).

Update or archive, not delete, still applies to:

- `docs/gap-data.md` — per `apps/STALE_DOC_DELETION_REGISTER.md` row for this doc, its `retrieval-eval-py` references should be updated to state plainly that `data-quality-go` is the active surface and the Python harness is unstarted, not "in progress."
- Any other doc that frames `retrieval-eval-py` as a scaffold-in-motion rather than a directory with zero content.

## Historical bottom line (superseded)

**Confirmed dead scaffolding, not live code, as of 2026-07-10.** `services/retrieval-eval-py/` contains zero files, has zero git history, and is wired into zero compose services, Makefile targets, or CI. `data-quality-go` is unambiguously the active retrieval-eval/quality surface for Data Plane v2 today. The right next decision, unchanged from the prior pass, is explicit:

- either create the Python harness for real (if the RAGAS/DeepEval-specific capability `docs/gap-data.md` §13.5 describes is actually wanted), or
- stop describing it as present or in-progress runtime state in any doc, and treat the empty directory as inert until someone acts on it.
