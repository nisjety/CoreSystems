# Recipe: add a gateway domain (velion-gateway-rs)

> How to add a new `/api/v1/*` surface to the Rust BFF at
> `apps/Frontend Plane/velionv3/apps/gateway`. The canonical reference is
> `src/domains/audit.rs` (read-only proxy) and `src/domains/inbox.rs`
> (read + mutate). Phase-1 domains added this way: `insights.rs`, `monitoring.rs`,
> `privacy.rs`, plus the `inbox.rs` AI-action routes.

## The honesty checklist (read before you ship)

A reviewer must be able to point at **any** value the SPA renders through this
domain and trace it to a real upstream response for a real org *today*, or see it
explicitly labelled. Before merging, confirm:

- [ ] **Org/identity comes from the session, never the client.** Use
      `upstream::authorized_org_id(&state, &user)` (or the validated
      `AuthenticatedUser.user_id`). Never read `x-velion-org-id` / a client query /
      a client body for scope. Add a test proving a spoofed scope is ignored.
- [ ] **Empty/unavailable is explicit, not zero-as-data.** An empty org, a 404, or
      an unreachable upstream returns an explicit empty/`unavailable` envelope —
      never a fabricated row, count, or status.
- [ ] **No affordance for something that does not run.** If the upstream feature
      isn't live, the surface says so (label/empty-state) rather than implying it
      works.
- [ ] **SSRF guard on every user-supplied URL** (`public_url::normalize_public_http_url`).
- [ ] **Secrets never leak downstream.** `proxy_json` / `proxy_bearer_json` inject
      the internal key or audience token; never echo them in a response.

## Steps

1. **Create `src/domains/<name>.rs`** with a `pub(crate) fn router(state) -> Router`
   that registers routes and ends with
   `.route_layer(axum::middleware::from_fn_with_state(state, require_session))`.

2. **Resolve scope server-side.** For org-scoped upstreams set `x-org-id` from
   `authorized_org_id` (empty org → return an empty success envelope, like
   `audit.rs`, rather than a 400 the SPA must special-case). For per-user
   upstreams build the path from `user.user_id` only (see `privacy.rs` — no client
   id is ever accepted). For Quarry-edge, mint the `quarry` audience token and send
   it as a bearer with **no** `x-org-id` (edge derives org from the JWT claim — see
   `monitoring.rs`).

3. **Normalize the upstream shape** in a private `fn` with a `#[cfg(test)]` shape
   test. Missing optional fields collapse to `null`/empty — never a fabricated
   value (see `monitoring::normalize_baseline`, `insights::normalize_connector`).

4. **Add config** if you call a new core: add the field to `AppState`
   (`src/config.rs`), set it from `env_url("<CORE>_URL", "http://<core>:<port>")`
   in `build_state`, **and add it to every `test_state`** (`src/main.rs` and
   `src/domains/chat/shared.rs`) or the test build won't compile. Add the env var
   to the gateway `docker-compose.yml` block.

5. **Register** the module in `src/domains/mod.rs` (alphabetical) and merge it in
   `build_router` (`src/main.rs`).

6. **Document the auth model** in a module doc-comment at the top of the file —
   state exactly where org/identity comes from and why there is no IDOR vector
   (every Phase-1 domain does this; copy the convention).

## Quality gates (gateway-crate-scoped — never `--workspace`)

```bash
cd "apps/Frontend Plane/velionv3/apps/gateway"
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```

## Deploy (local)

`docker compose build gateway` then `docker compose up -d --force-recreate --no-build gateway`
(from `apps/Frontend Plane/velionv3`). Note: `up --build` alone does **not** always
recreate the container onto the freshly-built image — force-recreate explicitly.
Then smoke the route live through `http://localhost:3185` with a real session
(`POST /api/v1/auth/sign-in`), asserting a real 2xx **and** a negative case
(spoofed scope ignored / 404 on a foreign id).
