# Delegated identity: implementation and deployment status (2026-09-29)

The recommended merge in `00-recommendation-security-review.md` §3 is implemented. It was built by workflow `delegated-identity-and-wave2-batch3` with an implementer and an adversarial reviewer.

## Built

| Service | What |
|---|---|
| auth-core | `POST /api/:audience/token-exchange` (RFC 8693 style). Subject audiences are hard-limited to `session-core`. Every exchanged token carries its own `jti` and is consumed exactly once by its resource server (integration-corev2). `zdr` is inherited from the subject on a code path distinct from `/internal-token`. TTL is 90 s. `tokenExchange` is validated in `parseRegistry`. |
| user-core | Lean `mail-task-v1` thread decision, minted from `resolveMailTaskAlias`. It refuses ZDR orgs and orgs that are not entitled to create threads. |
| session-core | `PrepareUserOwnedThreadRequest.control_decision_token` (field 5). The `mail_task` origin is verified field by field (org, user, session_key). The caller allow-list includes the bridge's distinct principal: the reviewer found it missing and fixed it. |
| execution-core | `mint_delegated_ingestion_token` for the four read paths, so the agent reads personal connections as the run's own user. One exchange per tool call, all with the run's single session-core bearer. A 403 from the exchange (registry not opted in) falls back to the old org-only token; any other failure fails closed. |
| model-gateway | `MailTaskBridgeTokenProvider`, a distinct principal with persistent session-core retention and a first-use `RetentionMisconfigured` check. Plus `POST /v1/internal/mail-task/prepare-thread`. |
| gateway BFF / integration-corev2 | `thread_decision` is passed through verbatim. `ThreadBridgeClient` is ready, and `MailTaskPipeline` stays `None` and answers 503. |

**Key tests:**
- `prepare_user_owned_thread_rejects_a_decision_minted_for_user_a_when_the_request_names_user_b` (mutation-checked);
- `two_concurrent_runs_for_two_different_users_never_cross_contaminate_the_delegated_read_credential`;
- exchanged-token single use: the `jti` is stamped by auth-core (`plane-token-exchange.composition.spec.ts`) and consumed once by integration-corev2 (`internal/auth/single_use_test.go`, `middleware_test.go`);
- one run bearer can be exchanged once per tool call, and a refused exchange does not burn it (`plane-token-exchange.composition.spec.ts`);
- the mail-task bridge principal can only prepare `mail_task`-origin threads (session-core and the model-gateway route both pin it);
- ZDR inheritance.

## Activation

**Agent reads the user's own personal connections.** Activated through the policy, with no hand-written secrets:
- `apps/Control Plane/config/plane-service-principals.json` gives `model-execution` the grant `"tokenExchange": {"ingestion": ["session-core"]}`.
- `apps/Control Plane/scripts/run-control-plane.sh` now copies `tokenExchange` into the assembled registry, but only when the policy declares it. Auth Core rejects an explicit `null`.
  - A dry run with fake credentials confirmed that only `model-execution` gets the key.
- It takes effect on the next launcher run from the Control plane, which regenerates `PLANE_SERVICE_PRINCIPALS_JSON` and restarts auth-core.

**Mail-to-task. Not activated, deliberately.** The path stays off until all of these exist:
1. **A Postmark account and receiving domain.** Owner action: set `MAIL_TASK_RECEIVING_DOMAIN` on user-core and integration-corev2, plus the webhook secret.
2. **Two new policy principals.** Add them only when (1) exists:
   - `model-gateway-mail-task-bridge`: aud `session-core`, scope `session:user-owned-thread-prepare`, retention `persistent`, consumerEnv `MAIL_TASK_BRIDGE_SERVICE_API_KEY`.
   - `verevon-gateway`: aud `model-gateway`, scope `mail-task:prepare-thread`, consumerEnv `MAIL_TASK_GATEWAY_BRIDGE_SERVICE_API_KEY`.
   - The launcher mints both credentials.
   - The two consumer env vars need compose passthroughs for model-gateway and the BFF.
3. **The `MailTaskPipeline` implementation itself**, which posts the synthesized turn and runs one draft-only execution turn. It is not built; the design scopes it out.

## Known limits

- The exchanged token's `jti` seen-set (integration-corev2 `auth/single_use.go`) is process-local. With several integration-corev2 replicas, a captured exchanged token could be replayed once per replica within its 90 s life. This is the same trade-off as the existing `ReplayCache`. The session-core subject token is deliberately NOT single-use: a run holds one bearer and exchanges it once per tool call, so a seen-set on it fails every read after the first.
- Trust narrative correction (review T-1): execution-core never verifies the session bearer, it forwards it opaquely. The bearer is checked at model-gateway ingress when the run is started, by session-core's run-owner check, and by auth-core's `verifyPlaneSubjectToken` at exchange time. Look there, not in execution-core, for the trust boundary.
- The model-gateway `RetentionMisconfigured` check runs on first use of a given org, not at process start, because the org is unknown at boot. `plane-service-principals-policy.contract.spec.ts` pins the persistent-retention requirement for the bridge principal as soon as its row is added to the policy.
- `internal/api/codex_subscription_direct_test.go` in integration-corev2 does not compile at HEAD (`declared and not used: access`, `undefined: auth`), so `go test ./internal/api/...` cannot run there. It is unrelated to this work and is owned by the codex-subscription work.
- `cargo clippy` has not been run on the new Rust code; only `check` and `test` have.
