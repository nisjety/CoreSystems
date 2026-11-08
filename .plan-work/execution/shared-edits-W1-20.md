## WS07-S7
shared-edit apps/Model Plane/go/services/capability-core/internal/learning/reviewer.go: in the anonymous struct of ParseReviewJSON add `Reason string `json:"reason"``; add `|| len([]rune(s.Reason)) > MaxReasonChars` to the candidate-invalid condition (or leave it unbounded, SelectForPersistence clamps to 300); set `Reason: strings.TrimSpace(s.Reason),` in the SkillCandidate literal.

## WS07-S7
shared-edit apps/Model Plane/go/services/capability-core/cmd/main.go: change `func startLearningConsumer(ctx, nc, sc, ic)` to take `opts ...sessionreview.Option` and pass them to `sessionreview.RunConsumer(ctx, js, sc, ic, model, opts...)`; at the call on line ~217 pass `sessionreview.WithMemoryControls(memoryClient)` only when memoryClient != nil (it comes from dialBackendsWithMemory on line 191; sessionCoreScopes already include session:memory-controls:read). Existing tests call startLearningConsumer with 4 args and keep compiling.

## WS07-S7
shared-edit apps/Model Plane/rust/services/execution-core/src/runtime_loop/mod.rs (reattach_skill, ~line 2446): filter the listed skills with `a.review_state.is_empty() || a.review_state == "accepted"` before recover_skill, as model-gateway tool_loop.rs now does; otherwise the owner's own proposals read as 'disabled' in the Do loop.

## WS07-S3
shared-edit apps/Frontend Plane/verevonv3/src/shared/actions/model-access.ts: right after the line "  'memory.delete': exempt('irreversible_destructive', 'destructive')," add exactly these three lines: "  'memory.set_state': exempt('human_identity_or_security', 'external_write')," then "  'memory.reset': exempt('irreversible_destructive', 'destructive')," then "  'memory.remember': exempt('human_identity_or_security', 'external_write'),". Without them MODEL_ACCESS lacks three ActionId keys, so pnpm typecheck fails and buildActionCatalogManifest breaks. With them, the 170 other frontend action tests pass.

