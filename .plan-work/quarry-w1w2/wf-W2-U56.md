# W2-U56 hand-over (WS13.8)

What this unit did: the contract docs (CONTRACTS section 10, RUNBOOKS section, ERROR_REASONS section, SCRAPE_CAPTURE status, OSS parity rows 22 and 23, DRIVER_MATRIX note, the stale-doc register section), the OpenAPI additions for scrape captures and actions, the opt-in e2e script and its make target, and two YAML fixes. Commits: 16fdb679e, 4d501698e, cb732988, 74dc1e6f, e30c7119b. Full status: W2-U56-progress.md. Bullets: depth/W2-U56.md.

Left: SDK regeneration (blocked: CreateScheduleRequest has duplicate keys catchup_window_s and pause_on_failure, WS15 content); the docs-contract gate file is absent from this worktree; SELF_HOST.md env rows; gateway actions passthrough (other plane's file set).

Traps found:
- docs/openapi.yaml at HEAD did not parse (two plain scalars with ": " from WS10.S3 and WS14.S2; duplicate keys from WS15). The generator reports one error at a time; each run is a docker container, so batch the checks.
- The working copy of docs/CONTRACTS.md lost the WS13.4b section (another agent's stale whole-file write). Do not commit that working copy whole; restore nothing by checkout.
- Shared worktree: openapi.yaml, CONTRACTS.md and RUNBOOKS.md working copies carry other units' hunks. Use qcommit-blobs.sh (HEAD blob plus your hunks), never qcommit.sh, for them.
- SDK generation reads the spec you give it: use a HEAD copy, or the working copy's uncommitted /v1/profiles hunks land in the SDK.
- make is not on this host (use the recipe by hand or a machine with make).
- Attribution: the session attribution reminder names Haiku 5.5 and the workflow brief asked for Sonnet 5.5; the commits carry Haiku 5.5.
