# wf-W2-U25 hand-over notes (WS10.S3)

What was done: the agent-run event trace (per-run seq, bounded replay ring, resume with Last-Event-ID / from_seq, replay_gap, close_run on finish), the env-only ring config, the gateway events proxy, the SPA typed reader, the OpenAPI path, the CONTRACTS subsection and the RUNBOOKS section. Details and commit list: W2-U25-progress.md.

Traps found (for the next blob commit):
- qcommit-blobs.sh stores bytes as given (no CRLF/LF normalisation). The repo stores LF. The first commit of the runtime files stored CRLF and needed a follow-up commit (no amend, no reset was allowed).
- qcommit-blobs.sh failed on a NEW file with "cannot add to the index"; use qcommit.sh for new files (it worked).
- Blob sources built from an older HEAD silently dropped other units' sections (RUNBOOKS). Build from the current HEAD right before committing and diff against HEAD.
- The docker lock queues qtest jobs behind other agents' full cargo checks (over 30 minutes once). Start the check early.
- The gateway working copy carried another unit's uncommitted edit to sanitize_action (a type error in the evaluate arm). It is not in this unit's commits.

Left: the test phase (run the tests named in the progress file), the deployment evidence in the RUNBOOKS section, WS10.S4 (trace_seq and the NATS exclusion), and the depth items in depth/W2-U25.md.
