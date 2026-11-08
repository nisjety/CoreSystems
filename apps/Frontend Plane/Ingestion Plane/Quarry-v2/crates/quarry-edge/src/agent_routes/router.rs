use super::*;

/// Agent routes, merged into the protected router so they inherit
/// `require_auth` (JWT → `Claims`). State is applied by the parent router.
pub fn agent_router() -> Router<AppState> {
    Router::new()
        .route("/v1/agent/runs", post(start_run))
        .route("/v1/agent/runs/:run_id/step", post(step))
        // `interact` is an alias for one governed action, preserving Quarry's one-step
        // receipt and approval semantics instead of adding an unbounded
        // macro executor.
        .route("/v1/agent/runs/:run_id/interact", post(step))
        .route(
            "/v1/agent/runs/:run_id/browser-session",
            get(browser_session),
        )
        .route(
            "/v1/agent/runs/:run_id/browser-session/control",
            post(set_browser_session_control),
        )
        .route(
            "/v1/agent/runs/:run_id/browser-session/timeline",
            get(browser_session_timeline),
        )
        .route(
            "/v1/agent/runs/:run_id/egress-receipts",
            get(egress_receipts),
        )
        .route("/v1/agent/runs/:run_id/receipts", get(list_receipts))
        .route(
            "/v1/agent/runs/:run_id/procedure",
            post(compile_run_procedure),
        )
        .route("/v1/agent/procedures/replay-check", post(replay_check))
        .route(
            "/v1/agent/procedures/quality-check",
            post(procedure_quality_check),
        )
        .route(
            "/v1/agent/procedures/impact-check",
            post(procedure_impact_check),
        )
        .route("/v1/agent/runs/:run_id/tabs", get(list_tabs).post(new_tab))
        // WS8.F: the agent attachment (dark behind QUARRY_EDGE__ATTACH) and the hand-over of a tab.
        .route("/v1/agent/runs/:run_id/attach", post(attach_run))
        .route("/v1/agent/runs/:run_id/attach-current-tab", post(attach_current_tab))
        .route(
            "/v1/agent/runs/:run_id/attach/:attach_id",
            delete(release_attach),
        )
        .route("/v1/agent/runs/:run_id/tabs/:tab_id/mark", post(mark_tab))
        .route(
            "/v1/agent/runs/:run_id/tabs/:tab_id/control",
            post(control_tab),
        )
        .route(
            "/v1/agent/runs/:run_id/tabs/:tab_id/select",
            post(select_tab),
        )
        .route("/v1/agent/runs/:run_id/tabs/:tab_id", delete(close_tab))
        .route("/v1/agent/runs/:run_id/devtools", get(devtools_events))
        .route("/v1/agent/runs/:run_id/frames/ws", get(live_frame_ws))
        .route(
            "/v1/agent/runs/:run_id/frames/stream",
            get(live_frame_stream),
        )
        .route("/v1/agent/runs/:run_id/frame", get(live_frame))
        // E3.3-h: the read-only point hit test. A query, not an action; dark behind
        // QUARRY_EDGE__HIT_TEST (default off).
        .route("/v1/agent/runs/:run_id/hit-test", get(hit_test_point))
        // W2 — single-shot live-view reference (no SSE). The
        // App Shell calls this on connect and again whenever
        // the `live_view` SSE event's `expires_at` elapses.
        .route("/v1/agent/runs/:run_id/live-view", get(live_view))
        // W3 — typed agent-event SSE alongside the frame
        // stream. The App Shell renders the action timeline
        // (`action.started` / `action.completed` / `action.failed`),
        // planner decisions (`observation.ready`), and run
        // lifecycle (`agent.started` / `agent.completed` /
        // `agent.failed`) on the agent console. The frame
        // stream is the live-view channel; this stream is the
        // typed-events channel. Both are scoped to the same run
        // and owned by the same caller, so they share the
        // tenant-scoping helper.
        .route(
            "/v1/agent/runs/:run_id/events/stream",
            get(agent_events_stream),
        )
        .route("/v1/agent/runs/:run_id", delete(close_run))
}
