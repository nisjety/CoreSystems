# wf-W2-U61: WS8.C broker side (hand-over, 2026-10-24)

What was done: see `W2-U61-progress.md` (same folder) for commits and checks, and `depth/W2-U61.md` for every deferred item.

Shape of the change (so a reviewer can find it):
- Wire: `apps/Model Plane/proto/model_plane/v1/browser.proto` fields 50-53 + `RenewGrant`; generated Go in `apps/Model Plane/go/gen/model_plane/v1`.
- Broker: `internal/grant/humanbrowse.go` (kinds' rules, planDerive, memory store), `postgres_humanbrowse.go` (PgStore), `policy.go`, `router.go`, `grant.go`/`postgres.go` (edited), `internal/server/human_browse.go` + `server.go`/`config.go`/`errors.go`, `internal/authz/authz.go`, `cmd/main.go`, `migrations/0004_browser_grants_policy.{up,down}.sql`.
- model-gateway: `browser_grants.rs`, one hunk in `browser_run.rs`.
- Contract and docs: `scripts/tests/browser-grant-contract.test.mjs`, Quarry `docs/RUNBOOKS.md` (appended section).

Traps found:
- The docker image `quarry-rust-env:2` has no `google/protobuf/*.proto`; the Model Plane workspace cannot build in it until `libprotobuf-dev` is installed in the command (about 20 s per run).
- `lib-edit.js` `sub` needs exact tab counts; reading a file through the Read tool shows the line number then one tab, so the first tab of a displayed line is not part of the file. A whitespace-insensitive helper (`scratchpad/lib-subw.js`, run gofmt after) avoided the miscounts for Go.
- Heredocs through this Bash tool lose backslashes in JS regex source; write JS with the Write tool.
- `gofmt` on CRLF files lists them as unformatted; format via `tr -d '\r' | gofmt` then convert back (`scratchpad/W2-U61/gofmtcrlf.sh`).
- Postgres evaluates CHECK constraints in name order; `TestPg_HumanBrowse_TableConstraints` relies on that for the "unknown kind" case (kind_check sorts before parent_check).

Open for the owner: the `.svc` deny-list addition, refusing a budget claim on human_browse, plain sessions not being extendable, sibling (b) sign-off on the browser_run.rs hunk (all listed in the depth file).
