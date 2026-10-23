# W2-U51 hand-over (WS13.7)

What was done: see `.plan-work/quarry-w1w2/W2-U51-progress.md`. Depth bullets: `.plan-work/quarry-w1w2/depth/W2-U51.md`.

## Draft RUNBOOKS section (to append to apps/Ingestion Plane/Quarry-v2/docs/RUNBOOKS.md once that file is free of other units' hunks)

### Scrape capture artifacts (WS13.7, gateway)
- Route: `GET /api/v1/knowledge/scrape-artifacts/{artifact_id}` on the Frontend gateway. Served only for png, jpeg, webp and application/pdf. A PDF is an attachment with CSP sandbox.
- Captures are requested by `POST /v1/knowledge/scrape` with `capture` (SPA) or `formats` containing `screenshot` / `pdf`. The cached scrape preview never carries captures.
- A ZDR org gets `browser_artifact_withheld` (404) and no upstream call.
- Another org's id, and a missing or aged-out artifact, get `scrape_artifact_not_found` (404).
- Rollback: revert the commits; Quarry-side capture stays behind `QUARRY_EDGE__SCRAPE_CAPTURE` (default off).

## Traps found (for the next agent)
- `lib-edit.js` writes a file as soon as its callback returns. A multi-file script that throws halfway leaves the earlier files written; re-running it applied `pub(super)` twice in browser.rs (`pub(super) pub(super) const`). Use one script per file, or check the file before re-running.
- The Write tool file is LF; quarry.rs, browser.rs and KnowledgeScrapePreview.tsx are LF in the working tree. `git` prints "LF will be replaced by CRLF" warnings for them; harmless.
- `cargo check` output in the shared gateway log can contain NUL padding; `tr -cd '\11\12\15\40-\176'` before grepping, or `--message-format short`.
- `tsc -b` from the root and `node node_modules/typescript/bin/tsc` both fail in this checkout (pnpm layout). Use `./node_modules/.bin/tsc -p tsconfig.app.json --noEmit`.
- Solid 2 in this app: `fireEvent.error` followed by a DOM assertion needs `waitFor`.
- Gateway `cargo check --tests` takes a few minutes on a cold native target; run it once, in the foreground with a long timeout.
- `rustfmt --check` on gateway files shows many pre-existing diffs (browser.rs, quarry.rs); do not use it as a gate on the whole file.

## Commit attribution
Commits use `Co-Authored-By: Claude Haiku 5.5 <noreply@anthropic.com>` per the attribution reminder in this session. The queue asked for the Sonnet line; the reminder took precedence because it is the session's own attribution rule.
