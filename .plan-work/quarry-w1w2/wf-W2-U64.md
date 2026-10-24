# Hand-over, W2-U64 (WS14.S5 static batch check)

Status: built to scope and committed locally (not pushed). Full ledger: `W2-U64-progress.md`. Depth: `depth/W2-U64.md`.

## Commits (branch quarryv2-next, local)

- 7edf11250 feat(quarry-edge): static check of batch URLs at handoff (WS14.S5)
- 55f614bc5 test(quarry-edge): batch handoff check over the wire (WS14.S5)
- aed6360dd feat(orchestrator): batch check context, blocked count and robots respect (WS14.S5)
- 832feaf08 docs(quarry): HandoffAck.rejected, batch 400 and the RUNBOOKS section (WS14.S5)
- plan-work commit: docs(plan-work): W2-U64 progress, depth and hand-over notes (WS14.S5), made after this file

## For the next agent

- Run the deferred tests in the test phase: `cargo test -p quarry-edge batch_check`; `cargo test -p quarry-edge --test batch_handoff_check`; `cd services/quarry-orchestrator && go test ./internal/workflows -run TestBatch -count=1` (PauseTwice needs a check against HEAD first).
- The WS14.S6 lane (uncommitted `crawl_request.rs`) is the owner of that name. When it lands, it may want to move `validate_batch_urls` next to its `FieldError`; `batch_check.rs` is the S5 home until then.
- Before WS14.S4 (robots gate) lands: `params.robots` is stamped `respect` on every batch, and `BatchJobWF` already sends the batch crawl context. The gate hooks in at the edge's `internal_run_page` for kind batch.
- Do not change edge observe-mode scope reporting without reading `depth/W2-U64.md` first (the high-severity crawl bug).
- Attribution: this unit used Haiku 5.5 on the commits (system reminder); the task text said Sonnet 5.5. The owner must confirm.

## Traps found

- `git show HEAD:path` applies the CRLF smudge filter on this Windows host, so a HEAD-based copy made from it has CRLF. Use `git cat-file blob HEAD:path` for copies that feed `qcommit-blobs*.sh`.
- The binary target of quarry-edge declares its own module tree (`main.rs`), so a new module used from `routes.rs` must be declared in `main.rs` as well as `lib.rs`.
- Go working copies are CRLF, so `gofmt -l` flags them; check LF copies (`tr -d '\r'`) instead.
