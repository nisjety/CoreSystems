# R2-agent-routes (round 3) - done
Commits: 486bbd28 (F5-1..F5-4 Quarry + tab in-flight guard), a6049bc2 (start_run_grants tests), 44450389 (gateway tab actor forwarding + takeover refusal + registry zod).
F5-1 decision: wire is camelCase `initiatedBy` (SPA browser-client.ts types and CONTRACTS say so); OpenAPI has no timeline schema.
F5-4 design: `?initiatedBy=` query on tab select/close, body field on open; gateway forwards only fixed "agent".
Traps: agent_routes/run_sweeper.rs carries R5's zdr-log-ok hunk (not committed by me). docs/CONTRACTS.md hunk 2 (WS3.J) not mine. action-registry.ts change not type-checked (no node_modules). Nothing in Model Plane sends actor "agent" for tab tools yet.
