## WS26-S6
shared-edit apps/Model Plane/rust/services/model-gateway/src/grpc.rs: line 3521 (cfg(test) mock MockInferenceOk analyze_language LanguageAnalysisResult literal): after `content_safety_json: String::new(),` add `injection_json: String::new(),`. Still missing as of this round; the lib test target does not compile without it.

## WS26-S6
shared-edit apps/Model Plane/deploy/docker-compose.yml: in the model-gateway environment, after CONTENT_SAFETY_SHADOW_SAMPLE_RATE (about line 308), add: CONTENT_SAFETY_SEMANTIC_INLINE: ${CONTENT_SAFETY_SEMANTIC_INLINE:-false}; CONTENT_SAFETY_MODE_DEFAULT: ${CONTENT_SAFETY_MODE_DEFAULT:-frame}; CONTENT_SAFETY_SEMANTIC_DEADLINE_MS: ${CONTENT_SAFETY_SEMANTIC_DEADLINE_MS:-2500}; CONTENT_SAFETY_SEMANTIC_MAX_CALLS_PER_RUN: ${CONTENT_SAFETY_SEMANTIC_MAX_CALLS_PER_RUN:-6}; CONTENT_SAFETY_INJECTION_MODEL: ${CONTENT_SAFETY_INJECTION_MODEL:-}.

## WS23-S4.code
shared-edit C:/dev/CoresSystem/.claude/worktrees/epic-shaw-4d613b/apps/Model Plane/rust/services/model-gateway/src/zdr_features.rs (CRLF, keep it): (a) invoke_field_names pattern: add `        connector_scope: _,` after `        retrieval_scope: _,` (line 223); (b) its vec!: add `        "connector_scope",` after `        "retrieval_scope",` (line 262); (c) FIELD_TREATMENTS: add `    ("connector_scope", refused_feature("connector_read")),` after the `("retrieval_scope", refused_feature("web_search")),` row (line 103), before the regenerated row; (d) refusal(): before `    if !request.tools.is_empty() {` (line 329) add `    if request.connector_scope.as_ref().is_some_and(|ids| !ids.is_empty()) {`, `        return feature("connector_read");`, `    }`. Items (a)-(c) sit beside retrieval_scope because InvokeRequest declares connector_scope there (http_routes.rs line 7910) and the file documents declaration order; the review's after-tools placement also passes the order test, but it breaks that documented order. Item (d) is what every_field_row_decides_what_its_treatment_says needs.

## WS23-S4.code
shared-edit C:/dev/CoresSystem/.claude/worktrees/epic-shaw-4d613b/apps/Model Plane/rust/services/model-gateway/tests/zdr_feature_table.rs (CRLF, keep it; not in the review's list): in cases(), after the retrieval_scope "auto" row (ends line 136) and before `("regenerated", ...)` (line 137), add `("connector_scope", with("connector_scope", json!(["conn-1"])), refused("connector_read"))` and `("connector_scope", with("connector_scope", json!([])), None)`, wrapped as rustfmt wants. Without the first row, every_field_has_a_row_that_matches_its_treatment fails at line 293 (connector_scope has no row in the ZDR spec) once item 1c lands. Read from source; not run.

## WS23-S4.code
shared-edit C:/dev/CoresSystem/.claude/worktrees/epic-shaw-4d613b/apps/Model Plane/rust/services/model-gateway/src/normalize.rs (CRLF, keep it): in make_request (line 504), add `            connector_scope: None,` after `            retrieval_scope: None,` (line 524).

## WS23-S4.code
shared-edit C:/dev/CoresSystem/.claude/worktrees/epic-shaw-4d613b/apps/Model Plane/rust/services/model-gateway/src/grpc.rs (sibling WS26-S6, not this slice): line 3513, the LanguageAnalysisResult literal lacks injection_json (field 14, uncommitted in inference.proto). Its owner adds `injection_json: String::new(),`. It blocks only the lib-test target; the lib fails on item 1 first.

