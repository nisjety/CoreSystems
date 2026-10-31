# SC1 L2 from a signed-in page: legs A to C and the restart-survival check

Unit W1-L2-steps, 2026-10-23. Not committed (it lives in `.plan-work`). Nothing in this file was run: it is a script for the person who signs in. Every "expected" below is read from code (file:line) or from the plan, and is marked [CODE] (read in code, not run) or [PLAN]. The first real run turns each [CODE] into an observed result in the table of section 8.

Who does what. The person opens the SPA of the stack under test in the built-in browser and signs in themselves (a seeded dev account, plan item l step 4: `local@verevon.dev` or `e2e@verevon.dev`). Nobody reads, asks for or pastes a password, token or cookie value. Every snippet below is a same-origin `fetch` with `credentials: 'include'` against a RELATIVE URL: the browser attaches the session cookie and the gateway (BFF) does the token mints. No snippet prints a response body, a token, a grant id or `allowed_domains`; they print status, the gateway's error code and message (fixed strings), and ids of sessions and runs.

Paths abbreviated: `GW` = `apps/Frontend Plane/verevonv3/apps/gateway/src`, `SPA` = `apps/Frontend Plane/verevonv3`, `QE` = `apps/Ingestion Plane/Quarry-v2/crates/quarry-edge/src`, `MP` = `apps/Model Plane`.

## 0. What a signed-in page can and cannot reach

The page reaches only what the Vite proxy forwards, `/api` and `/health` to the gateway (`SPA/vite.config.ts:65-75`). The grant contract has hops the page cannot reach: Auth Core's token route (`GW/audience_tokens.rs:102`, cross-origin to :3011), Model Gateway's `POST /v1/browser/grants` (called only by the gateway, `GW/domains/browser.rs:4296`), and Quarry's `/v1/agent/runs` directly. So the page cannot run the spec's legs A1 to A3, A5a, A5b, A5c and A6 (`SPA/tests/e2e/browser-grants-prod-contract.spec.ts:238-258, 345-418`) as written: those need a bearer for Auth Core, Model Gateway or Quarry. Through the BFF it reaches the same contract from the user's side:

| Spec leg (file:line) | Same-origin from the page? | Where it is proven otherwise |
|---|---|---|
| A1 three user tokens mint (`spec:238`) | No (Auth Core, cross-origin). Indirectly yes: A.1 below only works if the gateway minted all of them (`GW/domains/browser.rs:4296-4300`, `4216-4228`). | wrapper `scripts/tests/browser-grant-prod-contract.sh` (Playwright `request` fixture) |
| A2 grant issued (`spec:244`) | Indirectly: session start in strict mode returns 200 only if the grant was issued (`GW/domains/browser.rs:525-529`). The grant id is never visible to the page (`GW/domains/browser.rs:4369-4376`). | wrapper; broker log line `browser grant issued` (host side, section 7) |
| A3 Quarry start with grant, hostile client hosts (`spec:252`) | No: the BFF sends one host and no client list (`GW/domains/browser.rs:530-541`). | wrapper; `quarry-edge --test browser_grants_prod` `prod_start_run_with_active_grant_overrides_client_domains` |
| A4 step to granted host 200, other host 403 (`spec:260`) | Yes: leg A, steps A.3 and A.4 (via `/actions`, same Quarry `/step`). | |
| A5a no grant 403 `browser grant_id is required` (`spec:345`) | No (needs a start without `grant_id`; the BFF always sends one in strict). Strict itself is shown by B.1. | wrapper; `prod_start_run_without_grant_is_403` |
| A5b forged id 403 (`spec:352`) | No. | wrapper; `prod_*` tests |
| A5c revoked grant with a LIVE run, 403 not 5xx (G6) (`spec:360`) | No: the page cannot revoke a grant without also closing the run (`GW/domains/browser.rs:1236-1277`). | wrapper; `prod_step_with_revoked_grant_is_403_not_5xx`. Same-origin stand-in for "grant dead while the run is live": the expiry step A.6 (inactive, 403, not 5xx). |
| A5d expired grant 403 (`spec:381`) | Yes: A.6 (needs the broker TTL, 45 s on the contract stack). | |
| A6 other user's grant (`spec:400`) | Only as an optional probe with a second signed-in browser profile: A.7. | wrapper (second seeded account) |
| B gateway session starts with grant (`spec:285`) | Yes: leg B. | |
| C AI run through the gateway (`spec:303`) | Yes: leg C. | |
| A7 no host names in broker, model-gateway, gateway logs | Host side only (section 7). | wrapper log scan (`scripts/tests/browser-grant-prod-contract.sh` scan_hops) |

Two things in the existing spec do not match the gateway and will bite when legs B and C run with `CONTRACT_BC_GREEN=1`; they are recorded in `depth/W1-L2-steps.md`, not fixed here: the spec reads `body.data?.sessionId ?? body.data?.id` (`spec:295,297,311`) but the session start answers `data.session.id` (`GW/domains/browser.rs:2956-2957`); and it posts `allowed_domains` and `max_steps` (`spec:318`) to a body that is `camelCase` (`GW/domains/browser.rs:327-347`), so both are silently ignored. The snippets below use the real shapes.

## 1. Preconditions, stack and mode

Pick the stack first; the legs say which they need.

| Stack | Used for | Quarry mode | Grant mode | Broker TTL |
|---|---|---|---|---|
| Gate stack: `docker-compose.yml` + `docker-compose.production.yml` + `docker-compose.grant-contract.yml` (`apps/Ingestion Plane/`), plan SC1 | legs A, B, C: the SC1 exit run | `ENVIRONMENT=production`, `QUARRY_EDGE__REQUIRE_BROWSER_GRANTS=1` (`apps/Ingestion Plane/docker-compose.grant-contract.yml`, last key) | gateway `BROWSER_GRANT_ENFORCEMENT=strict` (default under production, explicit `off` refused: `GW/config.rs:464-490`) | 45 s (`browser-grant-prod-contract.sh`, `GRANT_TTL_SECONDS=45`; broker clamps 30-3600, `MP/deploy/docker-compose.yml:1627`) |
| Side-by-side or E1 stack (dev flags; the 2026-10-16 run used `qv2v-*` containers, `.plan-work/quarry-w1w2/quarryv2-verification-2026-10-16.md:12,37`) | leg C and the restart check where the TTL must be long | dev | whatever the stack sets; `strict` for B.1 | default 900 s |
| Live stack | NOT used for any step here. The restart is a live-stack action (section 6) and needs the owner's go-ahead per stack. | | | |

Host-side, read-only preflight before the first step (changes nothing; it checks quarry-edge's environment, the broker TTL, `strict`, read-only root, `cap_drop`, no published port, zero `--no-sandbox`, and the auth-core registry row):

```
bash scripts/tests/browser-grant-prod-contract.sh --preflight-only
```

Record its transcript lines under "mode evidence" in the results table. Container names (defaults of the wrapper, `browser-grant-prod-contract.sh:56-61`; override per stack with the same variable names): gateway `frontend-plane-verevonv3-gateway-1`, model-gateway `model-plane-model-gateway-1`, execution-core `model-plane-execution-core-1`, quarry-edge `quarry-edge`, broker `model-plane-browser-broker-1`, auth-core `auth-service`. On a `qv2v-*` side-by-side stack the broker is `qv2v-broker`, model-gateway `qv2v-mg`, edge `qv2v-edge`; the gateway, auth-core and execution-core names are whatever the stack runner started [name not recorded in the 2026-10-16 file for those three].

Page preconditions:
1. The SPA of the stack under test is open in the built-in browser and the user is signed in. The SPA origin must serve `/api` to THAT stack's gateway. (`:5173` is the main checkout's container; a worktree stack needs its own dev server, project memory "Verevon v3 worktree verification".)
2. The tab stays in the foreground during the waits (timers throttle in a background tab).
3. `example.com` is reachable from the stack (the broker requires dotted non-IP hosts and Quarry's SSRF guard blocks private targets, plan SC1 text, so no local fixture can stand in).
4. At most a few sessions are open: `browser_start` is 30 requests/min per organization (`GW/rate_limit.rs:48,185`) and Quarry has per-org session caps; every leg below closes what it opened.
5. The planner flags `QUARRY_BROWSER_AGENT_LLM` and `QUARRY_BROWSER_AGENT_PLANNER_MODE` stay unset on the gate stack (plan WS1.A2 deployment evidence, R-28) so leg C is LLM-free [PLAN]. On a stack with an LLM configured the run is the normal planner.

## 2. The helper (paste once per page load, in the DevTools console of the signed-in tab)

It defines `L2`. `L2.call` returns `{status, errorCode, errorMessage, data}`; the response body is kept in memory in a non-enumerable property and never printed. `L2.check` records PASS or FAIL. Nothing here reads cookies or storage.

```js
window.L2 = (() => {
  const results = [], rows = [];
  const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
  async function call(label, method, path, body) {
    const init = { method, credentials: 'include', headers: { Accept: 'application/json' } };
    if (body !== undefined) { init.headers['Content-Type'] = 'application/json'; init.body = JSON.stringify(body); }
    const t0 = performance.now();
    const row = { label, method, path, status: 0 };
    try {
      const res = await fetch(path, init);           // same origin, relative URL
      const text = await res.text();
      let json = null; try { json = JSON.parse(text); } catch { /* not JSON */ }
      const err = json && json.error;                // gateway envelope: {error:{code,message}}; model-gateway: {error:"..."}
      row.status = res.status;
      row.errorCode = err && typeof err === 'object' ? err.code : undefined;
      row.errorMessage = err ? (typeof err === 'object' ? err.message : String(err)) : undefined;
      Object.defineProperty(row, 'json', { value: json, enumerable: false });
    } catch (e) { row.transport = String(e && e.name); }
    row.ms = Math.round(performance.now() - t0);
    rows.push(row); console.log(label, method, path, row.status, row.errorCode ?? '', row.errorMessage ?? '', row.transport ?? '');
    return row;
  }
  const data = (row) => (row.json && (row.json.data ?? row.json)) || {};
  function check(name, ok, detail) { results.push({ name, ok: !!ok, detail: detail ?? '' }); console.log(ok ? 'PASS' : 'FAIL', name, detail ?? ''); return !!ok; }
  const grantLeak = (row) => /"grant_?[iI]d"/.test(JSON.stringify(row.json ?? {}));
  const show = () => { console.table(results); console.table(rows.map(({ label, status, errorCode, errorMessage, ms }) => ({ label, status, errorCode, errorMessage, ms }))); };
  return { results, rows, sleep, call, data, check, grantLeak, show, t0: new Date().toISOString(), s: {} };
})();
console.log('L2 ready, t0 =', L2.t0);
```

Keep `L2.t0` (it is the `--since` for the host-side log scan, section 7). The two messages Quarry returns that the legs assert (strings are exact; they come from `QE/agent_routes/admission.rs:43` and `admission.rs:430`, `lifecycle.rs:229`):

```js
L2.MSG = {
  outside: "browser navigation is outside the run's allowed domains",
  inactive: 'browser grant is inactive or expired',
  widening: 'browser grant is not authorized',   // model-gateway string error, forwarded as is (GW/domains/browser.rs:4683-4685)
};
```

Step zero, once, to prove the page is signed in without printing the user (the SPA's own probe, `SPA/src/shared/api/auth-client.ts:108-116`; 200 with a null body means signed out):

```js
const s0 = await L2.call('A.0 session probe', 'GET', '/api/v1/auth/session');
const u0 = L2.data(s0);
L2.check('A.0 signed in', s0.status === 200 && !!(u0.user || (s0.json && s0.json.user)), 'status=' + s0.status);
```
If A.0 fails, stop: sign in, rerun. A 401 anywhere below on a signed-in page is a FAIL of the leg (never "the known failure").

## 3. Leg A: the Quarry grant contract through the BFF

Purpose: with the production posture, a session started through the gateway runs under a broker grant for exactly the start host; a step to that host is 200, a step to any other host is refused with 403 for the host (not an authentication failure, not a 5xx); a grant past its TTL ends the next step with 403, not a 5xx; a closed session answers 4xx, not 5xx.

Preconditions: gate stack (production overlay + contract overlay, `strict`, TTL 45 s); A.6 needs the TTL value T from the preflight line `BROWSER_BROKER_GRANT_TTL_SECONDS`. A.1 to A.5 work on any stack that enforces grants; on a `permissive` or `off` stack they run but prove less (record the mode).

Steps (run in order; each is one console paste).

A.1 Start a session on example.com (gateway: issue grant, start Quarry run with `grant_id`, first `navigate`). Route `POST /api/v1/browser/sessions` (`GW/domains/browser.rs:399`), handler `create_session` (`:491`), body `CreateSessionBody` camelCase `{url, zdr?, viewport?, profileId?, persistentProfile?, scope?}` (`:107-129`); the SPA sends the same (`SPA/src/shared/api/browser-client.ts:378-389`). No org header is needed (the gateway drops client org headers, `GW/middleware.rs:76`).
```js
const a1 = await L2.call('A.1 start session', 'POST', '/api/v1/browser/sessions', { url: 'https://example.com' });
const d1 = L2.data(a1);
L2.s.A = { id: d1.session && d1.session.id };
L2.check('A.1 200 and a session id', a1.status === 200 && typeof L2.s.A.id === 'string' && L2.s.A.id.length > 0, 'status=' + a1.status);
L2.check('A.1 session live, not zdr', d1.session && d1.session.status === 'live' && d1.session.zdr === false);
L2.check('A.1 first observation present (the gateway took the first step)', !!d1.observation);
L2.check('A.1 no grant id in the response', !L2.grantLeak(a1));
```
Expected [CODE]: 200 `{data:{session:{id,status:'live',zdr:false,...},observation:{...}}}` (`GW/domains/browser.rs:613-620, 2956-2985`). In strict, any grant failure is a typed error instead and no run starts: 400 `browser_host_not_grantable`, 403 `browser_grant_denied`, 429 `browser_grant_limit_reached`, 503 `browser_grant_unavailable` (`GW/domains/browser.rs:4231-4259`). Assert on ids and status only. Record the status, `session.id`, `ms`.

A.2 Read it back (`GET /api/v1/browser/sessions/{id}`, `:408`, handler `get_session` `:626`; it returns Quarry's owner projection with the grant id stripped, `:4369-4376`):
```js
const a2 = await L2.call('A.2 get session', 'GET', '/api/v1/browser/sessions/' + encodeURIComponent(L2.s.A.id));
L2.check('A.2 200, same run, no grant id', a2.status === 200 && L2.data(a2).runId === L2.s.A.id && !L2.grantLeak(a2), 'status=' + a2.status);
```
Expected [CODE]: 200, `data.runId` equals the id (Quarry projection field `runId`, `GW/domains/browser.rs:2882-2886`).

A.3 Step to the granted host. Route `POST /api/v1/browser/sessions/{id}/actions` (`:414`), handler `run_action` (`:721`), body `{action:{type:'navigate',url}, actor}` (`ActionBody`, `:250-255`). Use `actor:'agent'` so the session stays in `agent_control` (a default `human` actor flips the session to `human_takeover` first, `:773-791`, which would disturb leg C).
```js
const a3 = await L2.call('A.3 step granted host', 'POST', '/api/v1/browser/sessions/' + encodeURIComponent(L2.s.A.id) + '/actions',
  { action: { type: 'navigate', url: 'https://example.com/' }, actor: 'agent' });
L2.check('A.3 200', a3.status === 200, 'status=' + a3.status);
```
Expected [CODE]: 200 with `data.session.id` equal to the id and `data.observation` set (`:806-820`). Mirrors spec A4 first assertion (`spec:263-264`).

A.4 Steps to hosts outside the grant. Both URLs pass the gateway's own check (`GW/public_url.rs:34-60`: public, http(s)); Quarry refuses them at the domain gate (`QE/agent_routes/admission.rs:425-432`, gate 11, before the navigation-security gate 12).
```js
for (const [n, u] of [['evil.example', 'https://evil.example/'], ['example.org', 'https://example.org/']]) {
  const r = await L2.call('A.4 step outside ' + n, 'POST', '/api/v1/browser/sessions/' + encodeURIComponent(L2.s.A.id) + '/actions',
    { action: { type: 'navigate', url: u }, actor: 'agent' });
  L2.check('A.4 ' + n + ' is 403 for the host', r.status === 403 && r.errorMessage === L2.MSG.outside, r.status + ' ' + (r.errorMessage ?? ''));
}
const a4b = await L2.call('A.4 the session survives the refusals', 'POST', '/api/v1/browser/sessions/' + encodeURIComponent(L2.s.A.id) + '/actions',
  { action: { type: 'navigate', url: 'https://example.com/' }, actor: 'agent' });
L2.check('A.4 granted host still 200 afterwards', a4b.status === 200, 'status=' + a4b.status);
```
Expected [CODE]: 403 with the message `browser navigation is outside the run's allowed domains`; Quarry error code `SECURITY_BLOCKED` (`QE`, `apps/Ingestion Plane/Quarry-v2/crates/quarry-core/src/error.rs:88`). The gateway forwards the Quarry envelope unchanged (`GW/domains/browser.rs:4683-4685`), so `errorCode` is `SECURITY_BLOCKED`; assert the message, record the code. A 401, a 5xx or any other message is a FAIL. The last check (survives) is the one assertion here that is the spec author's expectation, not a plan sentence: if it fails, record it as a finding, do not weaken A.4.

A.5 Close (revoke) and then a step on the closed session. Route `DELETE /api/v1/browser/sessions/{id}` (`:463-465`), handler `close_session` (`:1236`): it reads the grant id from the owner projection, closes the Quarry run, then revokes the grant at the broker best effort (`:1236-1277`); the SPA's wrapper is `closeBrowserSession` (`browser-client.ts:537-545`).
```js
const a5 = await L2.call('A.5 close', 'DELETE', '/api/v1/browser/sessions/' + encodeURIComponent(L2.s.A.id));
L2.check('A.5 close is 200 closed:true', a5.status === 200 && L2.data(a5).closed === true, 'status=' + a5.status);
const a5b = await L2.call('A.5 step after close', 'POST', '/api/v1/browser/sessions/' + encodeURIComponent(L2.s.A.id) + '/actions',
  { action: { type: 'navigate', url: 'https://example.com/' }, actor: 'agent' });
L2.check('A.5 a closed session answers 4xx, never 2xx or 5xx', a5b.status >= 400 && a5b.status < 500, 'status=' + a5b.status);
```
Expected [CODE]: close 200 `{data:{closed:true}}` (`:1277`); the follow-up 404 `agent run not found` (`QE/agent_routes/types.rs:507-514`) or, if Quarry holds a resumable checkpoint for the run, a typed 4xx naming the resume id (`:515-516`). Record the exact status and message. Not provable here: that the broker now refuses the revoked grant with 403 on a live run (see section 0, A5c).

A.6 Expiry. Needs T (preflight). Start a fresh session, step once (200), wait T + 5 s, then step: the grant is past its TTL, the broker answers inactive, Quarry answers 403 (not a 5xx). Mirrors spec A5d (`spec:381-397`). On a 900 s stack the wait is 905 s; run it there only if the exit gate needs it.
```js
const T = 45;   // set to the broker's BROWSER_GRANT_TTL_SECONDS from the preflight
const a6 = await L2.call('A.6 start', 'POST', '/api/v1/browser/sessions', { url: 'https://example.com' });
L2.s.A6 = { id: L2.data(a6).session && L2.data(a6).session.id };
L2.check('A.6 started', a6.status === 200 && !!L2.s.A6.id, 'status=' + a6.status);
const a6s = await L2.call('A.6 step before expiry', 'POST', '/api/v1/browser/sessions/' + encodeURIComponent(L2.s.A6.id) + '/actions',
  { action: { type: 'navigate', url: 'https://example.com/' }, actor: 'agent' });
L2.check('A.6 step before expiry is 200', a6s.status === 200, 'status=' + a6s.status);
console.log('waiting', T + 5, 's'); await L2.sleep((T + 5) * 1000);
const a6e = await L2.call('A.6 step after expiry', 'POST', '/api/v1/browser/sessions/' + encodeURIComponent(L2.s.A6.id) + '/actions',
  { action: { type: 'navigate', url: 'https://example.com/' }, actor: 'agent' });
L2.check('A.6 403 for the grant, not a 5xx', a6e.status === 403 && a6e.errorMessage === L2.MSG.inactive, a6e.status + ' ' + (a6e.errorMessage ?? ''));
const a6r = await L2.call('A.6 AI run on an expired session', 'POST', '/api/v1/browser/sessions/' + encodeURIComponent(L2.s.A6.id) + '/ai-runs', { goal: 'Open example.com.' });
L2.check('A.6 AI run is 409 browser_session_grant_inactive', a6r.status === 409 && a6r.errorCode === 'browser_session_grant_inactive', a6r.status + ' ' + (a6r.errorCode ?? ''));
await L2.call('A.6 cleanup close', 'DELETE', '/api/v1/browser/sessions/' + encodeURIComponent(L2.s.A6.id));
```
Expected [CODE]: step 403 `browser grant is inactive or expired` (`QE/agent_routes/admission.rs:33-45`, called per step at `admission.rs:382`, per-step revalidation; `NotFound` and `FailedPrecondition` from the broker become inactive, `.plan-work/quarry-w1w2/A2b-gates-and-prod-tests-progress.md` item 2); AI run 409 with code `browser_session_grant_inactive` (`GW/domains/browser.rs:1641-1651`, `session_grant_inactive` `:4465-4472`, 409 case needs the upstream `browser_grant_not_derivable`, `MP/rust/services/model-gateway/src/browser_run.rs:772-780`). If the AI-run answer is something else (for example the projection read fails first), record the status and code and mark the line [CODE] unconfirmed.

A.7 Optional: another user's session (needs a SECOND signed-in browser profile; the first user's session id is copied by hand, it is not a secret but never reuse the signed-in cookie of the first user in the second profile). In the second profile, after pasting the helper and A.0:
```js
const SID = '<paste the id from A.1 of the first profile; start a fresh session there first, it was closed in A.5>';
const a7 = await L2.call('A.7 other user reads the session', 'GET', '/api/v1/browser/sessions/' + encodeURIComponent(SID));
const a7b = await L2.call('A.7 other user steps', 'POST', '/api/v1/browser/sessions/' + encodeURIComponent(SID) + '/actions', { action: { type: 'navigate', url: 'https://example.com/' }, actor: 'agent' });
L2.check('A.7 neither is 2xx or 5xx', [a7.status, a7b.status].every((s) => s >= 400 && s < 500), a7.status + ' ' + a7b.status);
```
Expected [CODE]: 403 or 404 (Quarry owner check, `QE/agent_routes/lifecycle.rs:6-34` for a grant of another user; an unknown run id for the caller is `agent run not found`). Close the first profile's session afterwards.

What to record for leg A: for each of A.0 to A.7 the PASS/FAIL line, status, `errorCode`, `errorMessage`, `ms` (from `L2.show()`), and the host-side scan (section 7). Cleanup: A.1's session is closed by A.5; A.6's by its cleanup line; if a step threw before its close, run `DELETE /api/v1/browser/sessions/{id}` for every id in `L2.s` (the close is idempotent, `GW/domains/browser.rs:1236-1277`).

Leg A passes when: every check above is PASS on the gate stack, the host-side scan has zero refusal hits on the happy path (`A.0` to `A.4`), and no response in `L2.rows` has status 5xx.

## 4. Leg B: a gateway session starts with a grant and takes its first step

Purpose: SC1's "UI session starts, acts and closes with zero 401/403 and zero grant-less fallbacks", and the strict-mode default. Plan WS1.B deployment evidence: `POST /api/v1/browser/sessions` 200, one `AcquireGrant` in the broker log, close revokes (plan line 665).

Preconditions: gate stack for the SC1 row. B.1 only means something when the gateway is in `strict`.

B.1 Strict proof from the page: a start host the broker cannot grant is refused BEFORE any Quarry run (in `permissive` the session would open with no grant, in `off` also). A public IP literal is the documented non-grantable host (plan WS1.B Risks: "IP-literal public hosts fail in strict (`browser_host_not_grantable`)"; broker policy requires dotted non-IP hostnames, `MP/rust/services/model-gateway/src/browser_run.rs:848-856`). It passes the gateway's public-URL check (`GW/public_url.rs:48-57`).
```js
const b1 = await L2.call('B.1 non-grantable host', 'POST', '/api/v1/browser/sessions', { url: 'https://1.1.1.1/' });
L2.check('B.1 strict refuses: 400 browser_host_not_grantable', b1.status === 400 && b1.errorCode === 'browser_host_not_grantable', b1.status + ' ' + (b1.errorCode ?? ''));
if (b1.status === 200) { // not strict: it opened a grant-less or off-mode session; close it and record the stack as NOT STRICT
  await L2.call('B.1 cleanup', 'DELETE', '/api/v1/browser/sessions/' + encodeURIComponent(L2.data(b1).session.id));
}
```
Expected [CODE]: 400 `browser_host_not_grantable` ("This address cannot be opened in the in-app browser.", `GW/domains/browser.rs:4235-4240`). On a production-posture stack, a grant-less session in `permissive` would instead come back 403 `browser grant_id is required` from Quarry (forwarded). Record which of the three outcomes appeared; only 400 `browser_host_not_grantable` proves `strict`.

B.2 Start, first step, close, in one paste (the spec's B, `spec:285-301`, with the real response shape):
```js
const b2 = await L2.call('B.2 start', 'POST', '/api/v1/browser/sessions', { url: 'https://example.com' });
const id2 = L2.data(b2).session && L2.data(b2).session.id;
L2.check('B.2 200 with a session id and the first observation', b2.status === 200 && !!id2 && !!L2.data(b2).observation, 'status=' + b2.status);
const b2t = await L2.call('B.2 tabs', 'GET', '/api/v1/browser/sessions/' + encodeURIComponent(id2) + '/tabs');
L2.check('B.2 tabs 200', b2t.status === 200, 'status=' + b2t.status);
const b2c = await L2.call('B.2 close', 'DELETE', '/api/v1/browser/sessions/' + encodeURIComponent(id2));
L2.check('B.2 close 200', b2c.status === 200, 'status=' + b2c.status);
```
Routes: `/tabs` is `GET` at `GW/domains/browser.rs:422-424`. Expected [CODE]: 200, 200, 200. Host-side: exactly one broker `browser grant issued` line (kind session) and one `browser grant revoked` per session in the window, zero in the gateway log (section 7).

B.3 A ZDR session (Zero Data Retention grant: memory-only `z_` id in the broker, plan X-2). Needed for the restart check; here it proves the BFF passes `zdr` into the grant.
```js
const b3 = await L2.call('B.3 start zdr', 'POST', '/api/v1/browser/sessions', { url: 'https://example.com', zdr: true });
const id3 = L2.data(b3).session && L2.data(b3).session.id;
L2.check('B.3 200, session zdr:true', b3.status === 200 && L2.data(b3).session && L2.data(b3).session.zdr === true, 'status=' + b3.status);
const b3s = await L2.call('B.3 step', 'POST', '/api/v1/browser/sessions/' + encodeURIComponent(id3) + '/actions', { action: { type: 'navigate', url: 'https://example.com/' }, actor: 'agent' });
L2.check('B.3 step 200', b3s.status === 200, 'status=' + b3s.status);
await L2.call('B.3 close', 'DELETE', '/api/v1/browser/sessions/' + encodeURIComponent(id3));
```
Expected [CODE]: 200 with `session.zdr === true` (`GW/domains/browser.rs:540, 2985`); `zdr:true` with `profileId` or `persistentProfile` is 400 `zdr_persistent_profile_forbidden` (`:512-514`, not tested here). Host-side: the broker line for a ZDR grant carries `zdr:true` and no grant id (`browser-broker/internal/server/grantlog.go:34-41`).

What to record for leg B: the three outcomes of B.1, ids and statuses of B.2 and B.3, `ms` of B.2's start (plan Risks: three extra round trips, about 100 to 150 ms, to be measured in L2), host-side counts. Cleanup: the closes above.

Leg B passes when every check is PASS and the log counts match (one issued and one revoked per session).

## 5. Leg C: an AI run through the gateway acts and observes with no 401 or 403

Purpose: user bearer to gateway to Model Gateway `DeriveRunGrant` to execution-core to Quarry: a run grant is derived from the session grant, the run acts and observes, and no refusal appears. Plan WS1.B deployment evidence (plan line 665): `ai-runs` 200 `run_id`, run events contain `BrowserActionDispatched` and `BrowserObservationReceived`, broker log has one `AcquireGrant` and one `DeriveRunGrant`.

Preconditions: gate stack; WS1.B on the stack (the derive route). Leg C runs an AI run: on a stack with no LLM configured and the deterministic planner (item 5 of section 1) the run is LLM-free; otherwise it needs the stack's model provider. Record which.

C.1 Start a session, then the AI run. Route `POST /api/v1/browser/sessions/{id}/ai-runs` (`GW/domains/browser.rs:403-406`), handler `start_ai_run` (`:1497`), body `StartAiRunBody` camelCase `{goal, allowedDomains?, maxSteps?, maxRuntimeS?, stopCriteria?, maxCostUsd?}` (`:327-347`); the SPA's client is `startBrowserAiRun` (`SPA/src/shared/api/browser-run-client.ts:137-165`). The answer is `{data:{run_id,thread_id,plan_id}}` (`browser-run-client.ts:112-121`). Omit `allowedDomains`: the run grant is derived from the session grant's single host; naming hosts can only narrow, and a 403 with named hosts is read as a widening refusal (`:4457-4472`).
```js
const c1 = await L2.call('C.1 start session', 'POST', '/api/v1/browser/sessions', { url: 'https://example.com' });
L2.s.C = { session: L2.data(c1).session && L2.data(c1).session.id };
L2.check('C.1 session started', c1.status === 200 && !!L2.s.C.session, 'status=' + c1.status);
const c2 = await L2.call('C.2 start AI run', 'POST', '/api/v1/browser/sessions/' + encodeURIComponent(L2.s.C.session) + '/ai-runs',
  { goal: 'Open example.com and read its title.', maxSteps: 3 });
const d2 = L2.data(c2);
L2.s.C.run = d2.run_id ?? d2.runId;
L2.check('C.2 200 with a run id', c2.status === 200 && !!L2.s.C.run, 'status=' + c2.status + ' ' + (c2.errorCode ?? c2.errorMessage ?? ''));
```
Expected [CODE]: 200 (`:1653-1668`). Refusals the gateway types: 409 `browser_session_grant_missing` (session had no grant; `:1554-1572`), 409 `browser_session_grant_inactive` (session grant expired or revoked; `:1641-1651`), 503 `delegated_auth_unavailable` (`:1585-1612`), 400 `invalid_browser_goal` (`:1513-1523`). Any 401 or 403 here is a FAIL of leg C.

C.3 Poll the run's durable event replay until both event types appear (route `GET /api/v1/runs/{run_id}/events/replay`, `GW/domains/chat.rs:77-80`, handler `run_events_replay` `GW/domains/chat/streams.rs:226-273`; the SPA reader is `listRunEventReplay`, `SPA/src/shared/api/run-console-client.ts:449-478`, response `{data:{runId,events:[{eventId|event_id,eventType|event_type,at|ts,payload}],nextEventId,truncated}}`). Only event TYPES and counts are kept; payloads are not printed.
```js
const types = (r) => ((L2.data(r).events) || []).map((e) => e.eventType ?? e.event_type);
let seen = [], last;
for (let i = 0; i < 30; i++) {              // up to about 90 s
  last = await L2.call('C.3 replay', 'GET', '/api/v1/runs/' + encodeURIComponent(L2.s.C.run) + '/events/replay?limit=200');
  if (last.status !== 200) break;
  seen = types(last);
  if (seen.includes('BrowserObservationReceived')) break;
  await L2.sleep(3000);
}
L2.check('C.3 replay readable', last.status === 200, 'status=' + last.status);
L2.check('C.3 BrowserActionDispatched seen', seen.includes('BrowserActionDispatched'));
L2.check('C.3 BrowserObservationReceived seen', seen.includes('BrowserObservationReceived'));
const refusal = /\b40[13]\b|PermissionDenied|Unauthenticated|not authorized/.test(JSON.stringify(L2.data(last).events ?? []));
L2.check('C.3 no 401/403/PermissionDenied/Unauthenticated in the run events', !refusal);
console.log('event types seen:', [...new Set(seen)].join(','));
```
Expected [CODE, PLAN]: both event types present and no refusal text (spec C, `spec:325-336`; plan line 665). If the run ends without an observation (planner or provider), record the distinct event types seen and the last status; that is a FAIL of C.3, not a reason to weaken it.

C.4 Widening negative (fresh session; one run per session): a start that names a host the session grant does not cover is refused.
```js
const c4s = await L2.call('C.4 start session', 'POST', '/api/v1/browser/sessions', { url: 'https://example.com' });
L2.s.C.session2 = L2.data(c4s).session && L2.data(c4s).session.id;
const c4 = await L2.call('C.4 widening request', 'POST', '/api/v1/browser/sessions/' + encodeURIComponent(L2.s.C.session2) + '/ai-runs',
  { goal: 'Open example.com.', allowedDomains: ['example.com', 'evil.example'] });
L2.check('C.4 403 browser grant is not authorized', c4.status === 403 && c4.errorMessage === L2.MSG.widening, c4.status + ' ' + (c4.errorMessage ?? ''));
```
Expected [CODE]: 403 `{error:"browser grant is not authorized"}` forwarded unchanged because hosts were named (`GW/domains/browser.rs:1641-1651, 4465-4471`; broker widening refusal is PermissionDenied which Model Gateway maps to 403, `MP/rust/services/model-gateway/src/browser_run.rs:772-777, 808-813`). A 409 `browser_grant_not_derivable` would be the plan's alternative mapping; record what appears.

C.5 Stop and cleanup:
```js
await L2.call('C.5 stop run', 'POST', '/api/v1/browser/runs/' + encodeURIComponent(L2.s.C.run) + '/control', { action: 'stop' });
for (const sid of [L2.s.C.session, L2.s.C.session2]) if (sid) await L2.call('C.5 close', 'DELETE', '/api/v1/browser/sessions/' + encodeURIComponent(sid));
```
Route `POST /api/v1/browser/runs/{run_id}/control` with `{"action":"pause"|"resume"|"stop"}` (`GW/domains/browser.rs:438-441, 1670-1673`). Record the statuses (a 2xx or a typed 4xx for an already-finished run are both fine; a 5xx is recorded).

Not provable here: "a later `/step` with the derived run grant returns 403 after the session is closed" (plan line 665) is a direct Quarry call; it stays with the wrapper.

What to record for leg C: `run_id` (id only), the distinct event types and their count, C.2 `ms`, the host-side log counts (broker: one session `AcquireGrant` and one `DeriveRunGrant` for the run; zero refusal lines on gateway, model-gateway, execution-core, quarry-edge, broker for the leg's window). Leg C passes when C.1 to C.4 are PASS and the host-side scan is clean for the happy window.

## 6. Restart-survival check (grant store in Postgres, WS1.C)

Purpose: plan WS1.C deployment evidence (plan line 681): a non-ZDR grant survives a broker restart; a ZDR grant is lost (the next step is 403 inactive, and the database row count did not increase); the broker, model-gateway and Quarry reconnect on their own. Restart semantics table of the ledger (`W1-work-breakdown.md` item c): non-ZDR grants survive, ZDR grants are lost with the next `/step` 403, broker unavailable is 5xx or 503 with no fail-open, Postgres unavailable fails `/readyz`.

Preconditions:
1. A stack whose broker TTL is long enough for the whole sequence: at least 300 s. The gate stack's 45 s TTL does NOT fit (a docker restart plus the migration step takes longer than the time left); run this check on the default-TTL (900 s) side-by-side or E1 stack, or on the gate stack rebuilt once with a longer TTL if the owner wants it recorded on the production overlay. The restart check does not need the production overlay.
2. The owner's go-ahead for this stack. The restart of the broker container is a live-stack action: never on the live stack (broker `model-plane-browser-broker-1`, project `model-plane`) without the owner saying so in chat; on the side-by-side stack it is `qv2v-broker`; on the gate stack it is the wrapper's `BROKER_CONTAINER` (default `model-plane-browser-broker-1` under that stack's own compose project). Name the container and the stack in the results table before you run it.
3. The broker is on the durable store: `docker logs <broker>` contains `browser-broker using the durable Postgres grant store; ZDR grants stay in memory` (`browser-broker/cmd/main.go:85`); `/readyz` answers `ok` (`cmd/main.go:97, 193-208`; published as 127.0.0.1:8087 in the Model Plane compose, `MP/deploy/docker-compose.yml:1618-1620`).
4. Host-side row counts (counts only, never rows: rows hold host lists). The broker's database is `browser_broker` on `ingestion-postgres` with a dedicated role (`MP/deploy/docker-compose.yml:1647`, `apps/Ingestion Plane/scripts/bootstrap_broker_db.sh`); the superuser role of that server is `ingestion_user` (`browser-broker/migrations/0001_browser_grants.up.sql` header). [UNVERIFIED whether `psql` inside the container needs a password: if it prompts, stop, do not type or look up a password; the owner supplies the count, or use the broker-log evidence only and say so in the table.]
```
docker exec ingestion-postgres psql -U ingestion_user -d browser_broker -tAc "select count(*) filter (where kind='session' and revoked_at is null and expires_at > now()) as live_sessions, count(*) as total from browser_grants"
```
Columns `kind`, `revoked_at`, `expires_at` are in the migration (`0001_browser_grants.up.sql:20-45`). On the shared server other users' rows count too: the check is the DELTA, so run it with no other browser activity on that stack.

Steps.

R.1 (page) open two sessions and step both. Keep this page open and do not reload (state is in `L2.s`; if the tab reloads, the ids are still in the results, paste them back).
```js
const r1 = await L2.call('R.1 start non-zdr', 'POST', '/api/v1/browser/sessions', { url: 'https://example.com' });
const r1z = await L2.call('R.1 start zdr', 'POST', '/api/v1/browser/sessions', { url: 'https://example.com', zdr: true });
L2.s.R = { ns: L2.data(r1).session && L2.data(r1).session.id, zs: L2.data(r1z).session && L2.data(r1z).session.id };
const stepR = (id, label) => L2.call(label, 'POST', '/api/v1/browser/sessions/' + encodeURIComponent(id) + '/actions', { action: { type: 'navigate', url: 'https://example.com/' }, actor: 'agent' });
const r1a = await stepR(L2.s.R.ns, 'R.1 step non-zdr before'), r1b = await stepR(L2.s.R.zs, 'R.1 step zdr before');
L2.check('R.1 both started and both steps 200', r1.status === 200 && r1z.status === 200 && r1a.status === 200 && r1b.status === 200, [r1.status, r1z.status, r1a.status, r1b.status].join(','));
console.log('restart now; started at', new Date().toISOString());
```
Expected [CODE]: four 200s.

R.2 (host) before the restart: run the row-count command above and write down `live_sessions` and `total` (call them L0, T0). Expected [CODE]: the non-ZDR session adds one live row, the ZDR session adds none (`TestPg_ZdrCreateWritesZeroRows`, WS1.C Tests list). So L0 = (live rows before step R.1) + 1 and T0 = (total before) + 1 (a run grant would add one more only for leg C; none is open).

R.3 (host, needs the go-ahead) restart the broker and wait until it is ready:
```
docker restart <broker container>
until [ "$(docker inspect -f '{{.State.Health.Status}}' <broker container>)" = healthy ]; do sleep 2; done
curl -s http://127.0.0.1:8087/readyz        # prints: ok   (adjust the host port per stack)
```
Do not step before it is healthy: during the gap Quarry's validator retries three times (50 ms, 200 ms) and then answers 5xx (`A2b-gates-and-prod-tests-progress.md` item 2), which is the documented "broker unavailable is 5xx or 503, never fail-open" row, and is a separate observation (R.5).

R.4 (host) row counts again: L1, T1. Expected [PLAN, CODE]: L1 = L0 and T1 = T0 (the non-ZDR row survived; the restart wrote no row; the ZDR grant never had one).

R.5 (page) steps after the restart:
```js
const r5a = await stepR(L2.s.R.ns, 'R.5 step non-zdr after restart');
const r5b = await stepR(L2.s.R.zs, 'R.5 step zdr after restart');
L2.check('R.5 non-zdr grant survived: 200', r5a.status === 200, 'status=' + r5a.status);
L2.check('R.5 zdr grant was lost: 403 inactive', r5b.status === 403 && r5b.errorMessage === L2.MSG.inactive, r5b.status + ' ' + (r5b.errorMessage ?? ''));
```
Expected [PLAN, CODE]: non-ZDR step 200 (the grant is read back from Postgres, `internal/grant/postgres.go`, `TestPg_SurvivesNewRepositoryInstanceOnSameDatabase`); ZDR step 403 `browser grant is inactive or expired` (the in-memory grant is gone, `NotFound` becomes inactive). Plan WS1.C: "after the restart `/step` returns 403 inactive and `SELECT count(*) FROM browser_grants` did not increase" (plan line 681). A 5xx on the first step after `healthy` is a FAIL (record the status, the time since healthy, and whether a second attempt 2 s later is 200: that is a reconnect-lag finding, not a pass).

R.6 (page) derive from a persisted parent (leg C after the restart): an AI run on the non-ZDR session works (Model Gateway derives from the Postgres parent); on the ZDR session it is refused.
```js
const r6a = await L2.call('R.6 AI run non-zdr after restart', 'POST', '/api/v1/browser/sessions/' + encodeURIComponent(L2.s.R.ns) + '/ai-runs', { goal: 'Open example.com and read its title.', maxSteps: 3 });
const r6b = await L2.call('R.6 AI run zdr after restart', 'POST', '/api/v1/browser/sessions/' + encodeURIComponent(L2.s.R.zs) + '/ai-runs', { goal: 'Open example.com and read its title.', maxSteps: 3 });
L2.check('R.6 non-zdr AI run is 200', r6a.status === 200, 'status=' + r6a.status + ' ' + (r6a.errorCode ?? ''));
L2.check('R.6 zdr AI run is refused 409 or 403, not 200 or 5xx', [403, 409].includes(r6b.status), 'status=' + r6b.status + ' ' + (r6b.errorCode ?? ''));
```
Expected [CODE]: 200; and for the ZDR parent that is gone, 409 `browser_session_grant_inactive` (no hosts named, `GW/domains/browser.rs:4465-4472`) or the same code via the 403 path. Stop the run (leg C.5 form) if R.6 started one.

R.7 (page) close both, host row counts a third time: L2c, T2c. Expected: the non-ZDR session's close revokes its row (`revoked_at` set), so `live_sessions` returns to L0 - 1 and `total` stays T0 (rows are purged later, `cmd/main.go:150-162`, rows expired more than 24 h); the ZDR close revokes nothing in Postgres.
```js
for (const id of [L2.s.R.ns, L2.s.R.zs]) await L2.call('R.7 close', 'DELETE', '/api/v1/browser/sessions/' + encodeURIComponent(id));
```
Cleanup: those two closes; confirm with the row command that `live_sessions` is back to its value before R.1. If the gate stack's TTL was changed for this check, restore it (that is a rebuild: owner's call).

Also record, for the ledger restart table: `docker logs <broker> --since <time of R.3>` count of the line `browser-broker using the durable Postgres grant store` (expect 1) and of `refusing to start` (expect 0).

## 7. Host-side log scan (counts only, read-only)

Run after leg A's happy steps, after leg B, after leg C, and once at the end; `SINCE` is `L2.t0` from the page. Patterns are the wrapper's (`browser-grant-prod-contract.sh:80-82`). Nothing prints a log line; it prints a number per hop.

```
SINCE='<L2.t0 as printed by the page>'
FAIL='\b40[13]\b|PermissionDenied|Unauthenticated|browser grant is not authorized|browser grant_id is required'
for c in frontend-plane-verevonv3-gateway-1 model-plane-model-gateway-1 model-plane-execution-core-1 quarry-edge model-plane-browser-broker-1; do
  printf '%s refusal-lines=%s\n' "$c" "$(docker logs --since "$SINCE" "$c" 2>&1 | grep -Ec -- "$FAIL")"
done
# host names must not be in the three hops the plan keeps domain-free (plan A7)
for c in model-plane-browser-broker-1 model-plane-model-gateway-1 frontend-plane-verevonv3-gateway-1; do
  printf '%s host-name-lines=%s\n' "$c" "$(docker logs --since "$SINCE" "$c" 2>&1 | grep -Ec -- 'example\.com|example\.org|evil\.example')"
done
# grant lifecycle counts at the broker
docker logs --since "$SINCE" model-plane-browser-broker-1 2>&1 | grep -c 'browser grant issued'
docker logs --since "$SINCE" model-plane-browser-broker-1 2>&1 | grep -c 'browser grant revoked'
```
Expected [CODE]: refusal lines 0 for the happy window (legs A.0 to A.4, B.2, B.3, C.1 to C.3); the negative steps (A.4 evil/example.org, A.6, C.4, R.5 ZDR) legitimately write refusal lines, so scan per window (wrapper does the same: happy phase scanned, negatives after). Host-name lines 0 on those three hops. `browser grant issued` and `browser grant revoked` come from `browser-broker/internal/server/grantlog.go:20-41`, which is an UNCOMMITTED file in this tree (git status at the start of this unit): the counts exist only on a broker image built after it is committed; until then count `AcquireGrant` or `DeriveRunGrant` in whatever the image logs and say which. The Quarry-side no-sandbox proof (`ps` zero `--no-sandbox`, a `navigate` 200) is the wrapper's `check_no_sandbox` after the happy legs (`browser-grant-prod-contract.sh:257`); A.1/A.3 are its `navigate` 200 on the page side.

## 8. Results table template (one copy per stack and per run)

Run header: date, operator, stack name and compose files, gateway image id and `SOURCE_REVISION`, quarry-edge image id and revision, broker TTL, `BROWSER_GRANT_ENFORCEMENT`, quarry-edge `ENVIRONMENT`, planner flags unset (yes/no), preflight transcript attached (yes/no), SPA origin used.

| Id | Step | Expected | Status seen | errorCode / message seen | PASS/FAIL | Notes (ms, ids) |
|---|---|---|---|---|---|---|
| A.0 | session probe | 200, user present | | | | |
| A.1 | start session | 200, session id, live, zdr false, observation, no grant id | | | | |
| A.2 | get session | 200, same run id, no grant id | | | | |
| A.3 | step example.com | 200 | | | | |
| A.4a | step evil.example | 403 outside-domains message | | | | |
| A.4b | step example.org | 403 outside-domains message | | | | |
| A.4c | step example.com after refusals | 200 | | | | |
| A.5 | close, then step | 200 closed:true; then 4xx (expect 404) | | | | |
| A.6 | expiry (T s) | before 200; after 403 inactive; AI run 409 browser_session_grant_inactive | | | | |
| A.7 | other user (optional) | 403 or 404 | | | | |
| B.1 | non-grantable host | 400 browser_host_not_grantable (strict) | | | | strict: yes/no |
| B.2 | start, tabs, close | 200, 200, 200 | | | | start ms |
| B.3 | zdr session | 200, zdr true, step 200 | | | | |
| C.1 | session | 200 | | | | |
| C.2 | AI run start | 200 run id | | | | ms |
| C.3 | replay | both event types, no refusal text | | | | types seen |
| C.4 | widening request | 403 browser grant is not authorized | | | | |
| C.5 | stop, close | 2xx or typed 4xx | | | | |
| R.2 | rows before restart | L0, T0 | | | | |
| R.3 | restart, healthy, readyz ok | healthy, ok | | | | seconds to healthy |
| R.4 | rows after restart | L1 = L0, T1 = T0 | | | | |
| R.5a | non-ZDR step after | 200 | | | | |
| R.5b | ZDR step after | 403 inactive | | | | |
| R.6a | non-ZDR AI run after | 200 | | | | |
| R.6b | ZDR AI run after | 409 (or 403) | | | | |
| R.7 | close, rows | live back to before R.1 | | | | |
| H.1 | refusal-line counts per hop, happy windows | 0 each | | | | |
| H.2 | host-name lines (broker, model-gateway, gateway) | 0 each | | | | |
| H.3 | broker issued / revoked counts | matches sessions opened / closed | | | | |
| H.4 | zero `--no-sandbox`, a `navigate` 200 on the same stack | 0; 200 | | | | wrapper line |

Verdict rule (plan SC1 and the baseline's rule, `W1-closeout-baseline.md` section 6): SC1 is PASS only if every clause is PASS at its evidence type; a [CODE] expectation that a run contradicts is recorded as FAIL with the observed status and message, never edited into the table. What this procedure cannot show is listed in section 0 and stays with the wrapper (A1 to A3, A5a to A5c, A6, A7's log scan beyond the three hops) and the `prod_*` tests.
