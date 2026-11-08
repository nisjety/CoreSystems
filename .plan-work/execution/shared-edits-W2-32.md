## WS25-S2
shared-edit C:/dev/CoresSystem/.claude/worktrees/epic-shaw-4d613b/apps/Model Plane/rust/services/execution-core/src/runtime_loop/park.rs: after line 531 (`        park_reason: parked.reason.as_str().to_owned(),`, 8 spaces) add `        loop_stop_code: String::new(),` (8 spaces); after line 1515 (`                park_reason: "provider_exhausted".to_owned(),`, 16 spaces) add `                loop_stop_code: String::new(),` (16 spaces). park.rs is LF. These two literals are the only RunAgentResponse literals in the Rust tree without the new field or update syntax. Without both, execution-core does not compile.

## WS25-S2
shared-edit C:/dev/CoresSystem/.claude/worktrees/epic-shaw-4d613b/apps/Model Plane/deploy/docker-compose.yml: execution-core service environment: add `LOOP_GUARD_MODE: ${LOOP_GUARD_MODE:-}` (unset means off; do not default it on).

## WS16-S5
shared-edit apps/Model Plane/proto/model_plane/v1/attention.proto (additive; each new message and field gets a one-line comment carrying (WS16 S5)): (a) line 23, the comment above rpc StreamAttention: replace 'The per-user attention stream. Not served yet (WS16 S5); the call answers UNIMPLEMENTED.' with 'The per-person attention stream (WS16 S5): a snapshot, then the changes, resyncs and heartbeats.' (b) in message ListAttentionResponse, after line 108 'AttentionCounts counts = 3;' add 'bool stream_enabled = 4;' with a comment saying whether this replica serves the stream. (c) replace everything from line 127 ('// Opens the caller's attention stream. Not served yet (WS16 S5).') to end of file with: StreamAttentionRequest{string org_id=1; string user_id=2}; StreamAttentionResponse{string epoch=1; uint64 seq=2; oneof frame{AttentionSnapshot snapshot=3; AttentionThreadChanged thread_changed=4; AttentionResync resync=5; AttentionHeartbeat heartbeat=6}}; AttentionSnapshot{repeated AttentionItem items=1; AttentionCounts counts=2}; AttentionThreadChanged{AttentionItem item=1; AttentionCounts counts=2}; AttentionResync{}; AttentionHeartbeat{}.

## WS16-S5
shared-edit apps/Model Plane/rust/services/session-core/src/main.rs: after line 14 (mod attention_grpc;) add the line mod attention_hub;

## WS16-S5
shared-edit apps/Model Plane/rust/services/session-core/src/attention.rs: in list() the literal pb::ListAttentionResponse { ... } starts at line 414; after line 417 (        counts: Some(counts),) add the line        ..Default::default(),

## WS20-S7
shared-edit apps/Application Plane/notification-core/internal/notification/service.go: replace lines 79-85 with: // nativeTypes are the content-free attention and scheduled-run outcome types. Native delivery is their\n// only channel in this build, so their preference gate reads the in-app channel alone.\nvar nativeTypes = map[string]struct{}{\n\t\"modelplane.needs_approval\":        {},\n\t\"modelplane.needs_input\":           {},\n\t\"modelplane.plan_ready\":            {},\n\t\"modelplane.scheduled_run_outcome\": {},\n}

## WS20-S7
shared-edit apps/Application Plane/notification-core/internal/runtime/native.go: after line 22 (\t\"kind\":        {},) insert the line: \t\"outcome\":     {},

## WS20-S7
shared-edit apps/Model Plane/go/services/capability-core/cmd/main.go: after line 640 (the closing brace of the CAPABILITY_CORE_ATTENTION_NOTIFY_ENABLED block, before the notifier, err := runwatch.NewNotifier line) insert:\n\t// WS20-S7: the scheduled-run outcome notice is off unless the switch is the literal \"true\".\n\tif os.Getenv(\"CAPABILITY_CORE_SCHEDULED_NOTIFY_ENABLED\") == \"true\" {\n\t\toptions = append(options, runwatch.WithScheduledOutcomeNotify())\n\t\tslog.Info(\"run-watch scheduled-run outcome notices enabled\")\n\t}

## WS20-S7
shared-edit docs/master-ownership-matrix.md (cited in the batch notes as apps/master-ownership-matrix.md, which does not exist): on rule 11 (line 168) replace `Ingestion support workers and the Frontend gateway own their client calls` with `Ingestion support workers, the Frontend gateway and Model Plane's capability-core (its run-watch, attention and scheduled-run outcome notices, each on its allowlisted type) own their client calls`.

