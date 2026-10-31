# D-D-B2-go hand-over (2026-10-27)

What was wrong, in order of weight
1. Production: `activities.RegisterRunInput.ScheduleID` had `json:"-"`. The workflow passes the input to the activity through the SDK's JSON data converter, so every ScheduledJobWF fire reached `RegisterScheduledRun` with an empty schedule id and was refused. Found only because `TestScheduledJobWF_HappyPathCrawl` runs the workflow in the Temporal test environment. Fixed (own tag; the HTTP body is a separate struct without the id).
2. Production: `RegisterScheduledRun` decoded control's answer as a bare object; control wraps every answer in `{request_id, data}`, so the job id was always empty ("control returned no job id"). Fixed (envelope or bare). Together with 1, no scheduled scrape, crawl or batch fire could ever have registered; the kill switch `QUARRY_SCHEDULED_JOBS_ENABLED` was off, which is why nothing live showed it.
3. D2-016: event seq was MAX+1 read and INSERT in two steps; a lost race became a silent drop (batch handler treats the unique violation as a replay). Now `EventLog.AppendNext` (advisory lock per run, same transaction).
4. Orchestrator `prepareSpec` mutated the caller's desired spec; the timeouts floor swallowed explicit overrides.
5. Test-side only: control tests read args from the tenant view (WS15.S1 changed the create response), decoded bare answers instead of the envelope; the overlap test compared an enum string the SDK prints differently; pause-twice never resumed its second pause.

Traps found
- A bare `cat > file` heredoc with no content hangs the Bash tool (it reads stdin); and a long quoted heredoc with backticks failed ("unexpected EOF"): write scripts with the Write tool.
- The Temporal test environment is a data-converter round trip: any `json:"-"` field on a workflow-to-activity input vanishes. Grep for `json:"-"` on activity inputs when a workflow test shows an empty field.
- `gofmt -l` lists every CRLF checkout file; it is not a signal here.
- Go tests are slow on this machine when other agents compile (a package took 40 s once); rerun before concluding anything.

Left for others: depth/D-D-B2-go.md. The Postgres tests of quarry-control need `QUARRY_TEST_DSN`; CI sets `QUARRY_REQUIRE_DB=1`, so TestPostgres_EventSeq runs there.
