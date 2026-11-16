- P1 test-gap: the in-file tests in agent_actions.rs that reach a guarded route (skill create, update, publish, restore and import; cron create and run-now; agent publish) mount no org-core posture stub, so they now answer 503 zdr_decision_unavailable, and the member refusals that expected a role 403 get the guard's answer first; owed: mount /api/v1/organizations/org-1 with org_zdr_posture(false) in each, as the spaces/mod.rs tests do about line 12073.
- P1 follow-up: scripts/tests/zdr-contract-sync.test.sh line 235 still pins the pre-W3-FIX-11 digest 2c5921b02a546023997e9f651e677c6544cecb6845020ce46aa2d200c2c9d7cc, and it must read "apps/Model Plane/api/contracts/space-surface.v1.json|6fb0226a4691f0f39811bae7a3171fa5e7691da5b4544596a8993c0544330ae6" (the new digest, recorded here; the test source is not edited under the build rules).
- P2 risk: durable_write_refusal reads the org posture through the 60-second cache (a NotZdr answer is cached), so an org that has just turned ZDR on can pass all 15 guards for up to a minute; zdr.rs org_posture_live (line 204) is the live read, a shared change outside this lock, carried from W3-DEC-ZDR.
- P2 deviation: create_space_agent (spaces/mod.rs) still reads no x-zdr header, because it takes no HeaderMap and its action caller dispatchers.rs:7116 is outside the lock, so a header alone does not refuse that route (carried from W3-DEC-ZDR; closing it needs that signature change).
- P2 decision: every guard runs after the membership check and before any role gate, token mint or upstream call (the save-draft order), so a caller with no active org now gets the gateway's 403 active_membership_required on a skill, plugin, schedule or trigger route where the upstream used to answer, and a ZDR org's non-admin gets the 412 rather than the role 403.
- P2 decision: the two typed trigger bodies carry the body's own zdr flag into the guard (new field zdr: Value, never forwarded), so a body zdr: true refuses as the header does.
- P2 gap: the model-gateway zdr_posture lint scans a fixed file list (http_routes.rs, browser_run.rs, browser_grants.rs, space_sse.rs, research_runs.rs and space_routes/), so event_trigger_routes.rs has no mg rows and is unlinted; its create and update refuse a ZDR claim through retention_gate, and its dry run refuses through dry_run_gate (event_trigger_dry_run_under_zdr_refused).
- P2 inconsistency: model-gateway's rows disagree with proxy_to_capability_core, which refuses every non-GET method for a ZDR claim (http_routes.rs 8702-8704): the mg DELETE rows for skills, plugins and cron say path-rule n/a ("deletes allowed under every posture"), yet those deletes are refused in practice, and the mg PATCH /v1/cron/:id row says refuse although capability-core's cron patch decodes only enabled and schedule_expr (workplane_apis.go patch), so update_cron stays unguarded as configuration (K4-14); the owner should settle both classes.
- P2 decision: run_cron_now and install_plugin are guarded as content writes, because the fire copies the stored template's title, description and config_json into a new tasks row (cron/sweeper.go InsertFire) and install writes child skills and MCP servers from the manifest; the model plane already refuses a ZDR claim on approve, unapprove, uninstall, draft discard and kill-list lift (mg rows, proxy guard), so those stay refused downstream.
- P2 risk: nothing was compiled, so the lead's compile pass should check the ? on durable_write_refusal's Option inside zdr_write_guard, that install_plugin and create_plugin_kill return the (StatusCode, Json) tuple their tails return while the other guarded handlers call into_response, and that publish_agent's HeaderMap follows Path; publish_skill, restore_skill_version and the delete classes rest on handler comments and the SQL statements read, not every statement (UNVERIFIED for the publish and restore SQL).
- P3 gap: the 14 new bff rows are linted in neither direction (the gateway lint scans only /api/v1/spaces routes, and nothing checks that a DurableWrite handler calls a guard), so the guard calls are verified by reading only.
- P3 deviation: details.surface is set on the 412 only (W3-FIX-18's label_zdr style), so the 503 zdr_decision_unavailable carries none, and the guard's membership 403 uses the generic message "An active organization membership is required to change this." on the routes that have no message of their own.

## Route survey: every non-GET route of agent_actions.rs

Classification: content (persists free text, or copies stored text into a new durable row) gets `zdr_write_guard` and a `bff:` DurableWrite/refuse row; ids, enums, deletes, configuration and dry runs are left. The save-draft guard (W3-DEC-ZDR) is included for completeness.

| # | Method and path | Handler | What it persists | Class | Gateway |
| --- | --- | --- | --- | --- | --- |
| 1 | POST /api/v1/skills | create_skill | skill text (`content`) | content | guarded (skills) |
| 2 | POST /api/v1/skills/{skill_id} | update_skill | skill text and fields, forwarded as PATCH | content | guarded (skills) |
| 3 | DELETE /api/v1/skills/{skill_id} | delete_skill | nothing (delete) | ids | left |
| 4 | POST /api/v1/skills/{skill_id}/publish | publish_skill | draft becomes the live text and a new version | content | guarded (skills) |
| 5 | DELETE /api/v1/skills/{skill_id}/draft | discard_skill_draft | nothing (drops the draft) | ids | left |
| 6 | POST /api/v1/skills/{skill_id}/versions/{version}/restore | restore_skill_version | stored old text copied into the draft | content | guarded (skills) |
| 7 | POST /api/v1/skills/import | import_skill | text of an uploaded SKILL.md or zip | content | guarded (skills) |
| 8 | POST /api/v1/plugins | create_plugin | manifest_json and description | content | guarded (plugins) |
| 9 | POST /api/v1/plugins/{plugin_id} | update_plugin | description (PATCH applies it) and state | content | guarded (plugins) |
| 10 | DELETE /api/v1/plugins/{plugin_id} | delete_plugin | soft delete | ids | left |
| 11 | POST /api/v1/plugin-catalog/{plugin_id}/approve | approve_plugin_catalog | catalog_approved flag | enum | left |
| 12 | POST /api/v1/plugin-catalog/{plugin_id}/unapprove | unapprove_plugin_catalog | catalog_approved flag | enum | left |
| 13 | POST /api/v1/plugins/{plugin_id}/install | install_plugin | child skills and MCP servers written from the manifest | content (derived) | guarded (plugins) |
| 14 | POST /api/v1/plugins/{plugin_id}/uninstall | uninstall_plugin | removes child rows (constant reason) | ids | left |
| 15 | POST /api/v1/plugin-kill-list | create_plugin_kill | free-text reason (up to 500 bytes) | content | guarded (plugins) |
| 16 | DELETE /api/v1/plugin-kill-list | delete_plugin_kill | removes the entry; audit row names digest and scope only | ids | left |
| 17 | POST /api/v1/cron | create_cron | name, description, task_template | content | guarded (schedules) |
| 18 | POST /api/v1/cron/{cron_id} | update_cron | enabled and schedule_expr only | config | left (see the P2 inconsistency) |
| 19 | DELETE /api/v1/cron/{cron_id} | delete_cron | nothing (delete) | ids | left |
| 20 | POST /api/v1/cron/{cron_id}/run | run_cron_now | new task copies the stored template's title, description, config_json | content (derived) | guarded (schedules) |
| 21 | PUT /api/v1/agents/{agent_id}/draft | save_agent_draft | draft name and system prompt | content | guarded (agent_builder; W3-DEC-ZDR, relabelled) |
| 22 | POST /api/v1/agents/{agent_id}/publish | publish_agent | immutable version copying the system prompt | content | guarded (agent_builder) |
| 23 | POST /api/v1/event-triggers | create_event_trigger | name, wake_filter_json | content | guarded (event_triggers) |
| 24 | PATCH /api/v1/event-triggers/{trigger_id} | update_event_trigger | name, wake_filter_json (the paths named) | content | guarded (event_triggers) |
| 25 | POST /api/v1/event-triggers/{trigger_id}/dry-run | dry_run_event_trigger | nothing recorded or fired (reads a stored webhook event) | ids | left (model plane refuses ZDR, dry_run_gate) |

Totals: 25 non-GET routes; 15 guarded (14 new, one already W3-DEC-ZDR); 10 left. The GET routes (lists, versions, draft read, preview, admitted actions, authoring) write nothing and are out of scope.

Downstream check: for the left routes, the model plane already refuses a ZDR claim on the proxied mg rows (approve, unapprove, uninstall, discard, kill-list lift) and on the dry run, so the gateway leaving them unguarded does not open a hole.

Registry rows added (`apps/Model Plane/api/contracts/space-surface.v1.json`, 286 rows in all): bff:POST:/api/v1/agents/{agent_id}/publish; bff:POST:/api/v1/skills; bff:POST:/api/v1/skills/{skill_id}; bff:POST:/api/v1/skills/{skill_id}/publish; bff:POST:/api/v1/skills/{skill_id}/versions/{version}/restore; bff:POST:/api/v1/skills/import; bff:POST:/api/v1/plugins; bff:POST:/api/v1/plugins/{plugin_id}; bff:POST:/api/v1/plugins/{plugin_id}/install; bff:POST:/api/v1/plugin-kill-list; bff:POST:/api/v1/cron; bff:POST:/api/v1/cron/{cron_id}/run; bff:POST:/api/v1/event-triggers; bff:PATCH:/api/v1/event-triggers/{trigger_id}. Every row is DurableWrite with zdr_class refuse and basis guard:durable_write_refusal.

Digest for scripts/tests/zdr-contract-sync.test.sh line 235: `apps/Model Plane/api/contracts/space-surface.v1.json|6fb0226a4691f0f39811bae7a3171fa5e7691da5b4544596a8993c0544330ae6`.
