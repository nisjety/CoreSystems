// SC1 L2 signed-in legs, packaged for the built-in browser pane (2026-10-29).
//
// Source procedure: .plan-work/quarry-w1w2/L2-signed-in-steps.md (unit W1-L2-steps, 2026-10-23),
// sections 0-8. Shapes re-read against the code on 2026-10-29 (main checkout):
//   GW = apps/Frontend Plane/verevonv3/apps/gateway/src
//   QE = apps/Ingestion Plane/Quarry-v2/crates/quarry-edge/src
//   MG = apps/Model Plane/rust/services/model-gateway/src
// Line numbers below are the 2026-10-29 ones; the 2026-10-23 doc's numbers for GW/domains/browser.rs
// have moved (routes now 461-536, create_session 563, run_action 899, close_session 1430,
// start_ai_run 1694, session_grant_refusal 4584, session_grant_inactive 4854).
//
// How to use: the lead passes this whole file as ONE javascript_tool call in the signed-in SPA tab.
// It defines window.L2 and returns "L2 ready ...". Then each leg is one call, for example
//   await L2.legA0()
// Every leg returns an array of {name, ok, detail}: ok true = PASS, false = FAIL,
// null = INFO / NOT-RUN (detail says which). L2.summary() returns the whole table as plain data.
//
// Rules this file keeps (section 0 and 2): same-origin fetch, relative URLs, credentials 'include';
// it never reads cookies or storage; it never prints or returns a response body, a token, a cookie,
// a grant id or allowed_domains. It returns statuses, the gateway's error code and message (fixed
// strings), timings, and ids of sessions and runs. Every leg closes what it opened, also when it throws.
//
// If a javascript_tool call times out while a leg is still running (legA6 waits TTL + 5 s), the
// leg keeps running in the page: poll L2.legStatus(), then read L2.legResult('<key>').
(() => {
  const prev = window.L2 && window.L2.__sc1 ? window.L2 : null; // re-injecting keeps the table

  // ---------------------------------------------------------------- section 2: the helper
  // L2.call returns {status, errorCode, errorMessage, ...}; the body is kept in memory in a
  // non-enumerable property and never printed. L2.check records PASS or FAIL.
  const results = prev ? prev.results : [], rows = prev ? prev.rows : [];
  const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
  const clip = (s, n = 160) => (s == null ? s : String(s).length > n ? String(s).slice(0, n) + '...' : String(s));
  async function call(label, method, path, body, opts) {
    const o = opts || {};
    const ctl = new AbortController();
    const timer = setTimeout(() => ctl.abort(), o.timeoutMs || 45000);
    const init = { method, credentials: 'include', headers: { Accept: 'application/json' }, signal: ctl.signal };
    if (body !== undefined) { init.headers['Content-Type'] = 'application/json'; init.body = JSON.stringify(body); }
    const t0 = performance.now();
    const row = { label, method, path, status: 0, at: new Date().toISOString() };
    if (o.cleanup) row.cleanup = true;
    try {
      const res = await fetch(path, init); // same origin, relative URL
      const text = await res.text();
      let json = null; try { json = JSON.parse(text); } catch { /* not JSON */ }
      const err = json && json.error; // gateway envelope {error:{code,message}}; model-gateway {error:"..."}
      row.status = res.status;
      row.errorCode = err && typeof err === 'object' ? err.code : undefined;
      row.errorMessage = err ? (typeof err === 'object' ? err.message : String(err)) : undefined;
      Object.defineProperty(row, 'json', { value: json, enumerable: false });
    } catch (e) { row.transport = String(e && e.name); }
    clearTimeout(timer);
    row.ms = Math.round(performance.now() - t0);
    rows.push(row);
    console.log(label, method, path, row.status, row.errorCode ?? '', clip(row.errorMessage) ?? '', row.transport ?? '');
    return row;
  }
  const data = (row) => (row.json && (row.json.data ?? row.json)) || {};
  function check(name, ok, detail) {
    results.push({ name, ok: !!ok, detail: detail ?? '' });
    console.log(ok ? 'PASS' : 'FAIL', name, detail ?? '');
    return !!ok;
  }
  // INFO (mode evidence, measurements) and NOT-RUN rows: ok is null, the detail names the kind.
  function note(name, detail, kind) {
    const k = kind || 'INFO';
    results.push({ name, ok: null, detail: k + ': ' + (detail ?? '') });
    console.log(k, name, detail ?? '');
    return null;
  }
  const grantLeak = (row) => /"grant_?[iI]d"/.test(JSON.stringify(row.json ?? {}));
  const show = () => {
    console.table(results);
    console.table(rows.map(({ label, status, errorCode, errorMessage, ms }) => ({ label, status, errorCode, errorMessage, ms })));
  };
  // One short line per response for a check's detail: status, code, message, transport, ms.
  const st = (r) => [
    r.status,
    r.errorCode ? r.errorCode : null,
    r.errorMessage ? '"' + clip(r.errorMessage) + '"' : null,
    r.transport ? 'transport=' + r.transport : null,
    r.ms + 'ms',
  ].filter((x) => x !== null && x !== undefined).join(' ');

  const L2 = prev || {};
  Object.assign(L2, {
    __sc1: 1, results, rows, sleep, call, data, check, note, grantLeak, show,
    t0: prev ? prev.t0 : new Date().toISOString(),
    s: prev ? prev.s : {},
  });
  L2.marks = prev && prev.marks ? prev.marks : {}; // window times for the host-side log scan (section 7)
  L2.open = prev && prev.open ? prev.open : {}; // session id -> leg key, while open
  L2.openRuns = prev && prev.openRuns ? prev.openRuns : {}; // AI run id -> leg key, while not stopped
  L2.legs = prev && prev.legs ? prev.legs : {};
  window.L2 = L2;

  // The messages the legs assert (exact strings).
  L2.MSG = {
    // QE/agent_routes/admission.rs:475 (gate 11), forwarded by older gateways unchanged. The current
    // gateway maps it to OUTSIDE_CODE (GW/domains/browser.rs:5093-5107), so A.4 accepts either.
    outside: "browser navigation is outside the run's allowed domains",
    // QE/agent_routes/admission.rs:44 (per-step revalidation) and lifecycle.rs:280.
    inactive: 'browser grant is inactive or expired',
    // MG/browser_run.rs:1044-1048 (string error), forwarded as is by forward_quarry_failure
    // (GW/domains/browser.rs:5108-5110) when the request named hosts.
    widening: 'browser grant is not authorized',
    OUTSIDE_CODE: 'browser_navigation_outside_scope',
  };

  // ---------------------------------------------------------------- shared step helpers
  const enc = encodeURIComponent;
  const sessPath = (id) => '/api/v1/browser/sessions/' + enc(id);
  L2.mark = (name) => { L2.marks[name] = new Date().toISOString(); return L2.marks[name]; };

  // POST /api/v1/browser/sessions (GW/domains/browser.rs:461, create_session :563). Body is camelCase
  // CreateSessionBody {url, profileId?, persistentProfile?, viewport?, zdr?, scope?} (:104-129).
  // Answer 200 {data:{session:{id,status,zdr,capabilities,...}, observation}} (:700, shape :3145-3260).
  async function startSession(label, body, leg) {
    const r = await call(label, 'POST', '/api/v1/browser/sessions', body);
    const d = data(r);
    const id = d.session && typeof d.session.id === 'string' && d.session.id.length > 0 ? d.session.id : null;
    if (id) L2.open[id] = leg;
    return { r, d, id };
  }
  // DELETE /api/v1/browser/sessions/{id} (route :536, close_session :1430-1475): reads the grant id from
  // the owner projection, closes the Quarry run, revokes the grant best effort; 200 {data:{closed:true}};
  // idempotent (a Quarry 404 is still 200 closed:true, :1471).
  async function closeSession(label, id, opts) {
    const r = await call(label, 'DELETE', sessPath(id), undefined, opts);
    if (r.status >= 200 && r.status < 300) delete L2.open[id];
    return r;
  }
  // POST /api/v1/browser/sessions/{id}/actions (run_action :899), ActionBody {action, actor} (:251-255).
  // actor 'agent' keeps agent_control (a 'human' actor flips the session to human_takeover first).
  const step = (label, id, url) => call(label, 'POST', sessPath(id) + '/actions', { action: { type: 'navigate', url }, actor: 'agent' });
  // POST /api/v1/browser/sessions/{id}/ai-runs (route :466, start_ai_run :1694), StartAiRunBody camelCase
  // {goal, allowedDomains?, maxSteps?, maxRuntimeS?, stopCriteria?, maxCostUsd?} (:390-410).
  // Answer 200 {data:{run_id,thread_id,plan_id}} (MG/browser_run.rs:1644-1648, wrapped by ok() :1866).
  async function startAiRun(label, sessionId, body, leg) {
    const r = await call(label, 'POST', sessPath(sessionId) + '/ai-runs', body, { timeoutMs: 60000 });
    const d = data(r);
    const run = r.status === 200 ? (d.run_id ?? d.runId ?? (d.data && (d.data.run_id ?? d.data.runId))) : undefined;
    if (run) L2.openRuns[run] = leg;
    return { r, run };
  }
  // POST /api/v1/browser/runs/{run_id}/control {"action":"stop"} (control_ai_run :1872).
  async function stopRun(label, run, opts) {
    const r = await call(label, 'POST', '/api/v1/browser/runs/' + enc(run) + '/control', { action: 'stop' }, opts);
    if ((r.status >= 200 && r.status < 300) || (r.status >= 400 && r.status < 500)) delete L2.openRuns[run];
    return r;
  }

  // Runs one leg: records its rows, closes what it opened (also on a throw), and adds the leg-wide
  // checks "no 401" (section 2: a 401 on a signed-in page is a FAIL of the leg) and "no 5xx"
  // (leg A pass rule, section 3; cleanup calls are recorded but not counted).
  async function runLeg(key, body) {
    const running = Object.entries(L2.legs).find(([, v]) => v.state === 'running');
    if (running) {
      return [{ name: key + ' guard', ok: false, detail: 'leg ' + running[0] + ' is still running; wait with L2.legStatus()' }];
    }
    const start = results.length, rowStart = rows.length;
    L2.legs[key] = { state: 'running', startedAt: new Date().toISOString() };
    try {
      await body();
    } catch (e) {
      check(key + ' threw (script error, not a stack answer)', false, String(e && e.name) + ': ' + clip(e && e.message));
    } finally {
      for (const [run, leg] of Object.entries(L2.openRuns)) if (leg === key) await stopRun(key + ' cleanup stop run', run, { cleanup: true });
      for (const [id, leg] of Object.entries(L2.open)) if (leg === key) await closeSession(key + ' cleanup close', id, { cleanup: true });
      const mine = rows.slice(rowStart);
      const left = Object.values(L2.open).filter((leg) => leg === key).length;
      if (left) check(key + ' cleanup closed everything it opened', false, left + ' session(s) still open: run L2.legA_close()');
      const u401 = mine.filter((r) => r.status === 401);
      check(key + ' no 401 in this leg', u401.length === 0, u401.map((r) => r.label).join('; '));
      const s5 = mine.filter((r) => r.status >= 500 && !r.cleanup);
      check(key + ' no 5xx in this leg', s5.length === 0, s5.map((r) => r.label + ' ' + r.status).join('; '));
      const t = mine.filter((r) => r.transport);
      if (t.length) note(key + ' transport errors (timeouts, network)', t.map((r) => r.label + ' ' + r.transport).join('; '));
    }
    const out = results.slice(start).map(({ name, ok, detail }) => ({ name, ok, detail }));
    L2.legs[key] = { state: 'done', startedAt: L2.legs[key].startedAt, endedAt: new Date().toISOString(), rows: out };
    return out;
  }
  L2.legStatus = () => Object.fromEntries(Object.entries(L2.legs).map(([k, v]) => [k, { state: v.state, startedAt: v.startedAt, endedAt: v.endedAt }]));
  L2.legResult = (key) => (L2.legs[key] && L2.legs[key].rows) || [{ name: key, ok: null, detail: 'NOT-RUN: no result for this leg yet' }];

  // ---------------------------------------------------------------- A.0: signed in
  // The SPA's own probe (SPA/src/shared/api/auth-client.ts:109-116; route GW/domains/auth.rs:34):
  // 200 with a user when signed in, 200 with a null body when signed out. If A.0 fails, stop: the
  // user signs in, the lead re-runs. Nothing about the user is printed.
  L2.legA0 = () => runLeg('A0', async () => {
    const s0 = await call('A.0 session probe', 'GET', '/api/v1/auth/session');
    const u0 = data(s0);
    check('A.0 signed in', s0.status === 200 && !!(u0.user || (s0.json && s0.json.user)), 'status=' + s0.status);
  });

  // ---------------------------------------------------------------- Leg A: A.1-A.5 (section 3)
  // Purpose: with the production posture, a session started through the gateway runs under a broker
  // grant for exactly the start host; a step to that host is 200, a step to any other host is 403 for
  // the host (not an auth failure, not a 5xx); a closed session answers 4xx, not 5xx.
  L2.legA_start = () => runLeg('A', async () => {
    L2.mark('A.happy.from');
    // A.1 start on example.com: the gateway issues the session grant, starts the Quarry run with
    // grant_id, takes the first navigate. Expected [CODE]: 200 {data:{session:{id,status:'live',
    // zdr:false},observation}}. In strict, a grant failure is a typed error and no run starts:
    // 400 browser_host_not_grantable, 403 browser_grant_denied, 429 browser_grant_limit_reached,
    // 503 browser_grant_unavailable (GW/domains/browser.rs:4584-4607).
    const a1 = await startSession('A.1 start session', { url: 'https://example.com' }, 'A');
    const d1 = a1.d;
    L2.s.A = { id: a1.id };
    check('A.1 200 and a session id', a1.r.status === 200 && !!a1.id, st(a1.r) + (a1.id ? ' id=' + a1.id : ''));
    check('A.1 session live, not zdr', !!d1.session && d1.session.status === 'live' && d1.session.zdr === false,
      d1.session ? 'status=' + d1.session.status + ' zdr=' + d1.session.zdr : 'no session');
    check('A.1 first observation present (the gateway took the first step)', !!d1.observation);
    check('A.1 no grant id in the response', !grantLeak(a1.r));
    // Mode evidence (WS8.D, newer than the 2026-10-23 doc): with BROWSER_HUMAN_BROWSE on
    // (GW/config.rs:216, 542-546, default off) the session gets a human_browse grant with no hosts
    // (GW/domains/browser.rs:597-609, 4634-4645) and the page sees capability 'human_browsing'
    // (:3267-3299). Then A.4 and B.1 cannot hold; SC1's gate stack must have it off.
    const caps = (d1.session && Array.isArray(d1.session.capabilities)) ? d1.session.capabilities : [];
    note('A.1 mode evidence', caps.includes('human_browsing')
      ? 'human_browsing capability PRESENT: BROWSER_HUMAN_BROWSE is on, open-web grant (A.4/B.1 expectations do not apply)'
      : 'human_browsing capability absent: start-host session grant (BROWSER_HUMAN_BROWSE off)');
    note('A.1 start ms', String(a1.r.ms));
    if (!a1.id) { note('A.2-A.5', 'no session id from A.1', 'NOT-RUN'); return; }
    const id = a1.id;

    // A.2 read it back: GET /api/v1/browser/sessions/{id} (route :470, get_session :804) returns
    // Quarry's owner projection with grantId/grant_id stripped (spa_owner_projection :4758-4764).
    // Expected [CODE]: 200, data.runId equals the id.
    const a2 = await call('A.2 get session', 'GET', sessPath(id));
    check('A.2 200, same run, no grant id', a2.status === 200 && data(a2).runId === id && !grantLeak(a2), st(a2));

    // A.3 step to the granted host. Expected [CODE]: 200 with data.session.id = id and data.observation
    // (run_action :899-1015). Mirrors spec A4 first assertion.
    const a3 = await step('A.3 step granted host', id, 'https://example.com/');
    check('A.3 200', a3.status === 200, st(a3));
    L2.mark('A.happy.to');

    // A.4 steps to hosts outside the grant. Both pass the gateway's public-URL check
    // (GW/public_url.rs:34-60); Quarry refuses them at the domain gate (QE/agent_routes/admission.rs:
    // 471-479, gate 11, before navigation security gate 12) with 403 SECURITY_BLOCKED + MSG.outside.
    // The current gateway maps that 403 to {code:'browser_navigation_outside_scope', message:'This
    // session is limited to the domains of its run.'} (GW/domains/browser.rs:5093-5107, test
    // forward_quarry_failure_maps_navigation_scope :5772). Accept either; a 401, a 5xx or any other
    // answer is a FAIL. The detail records which form appeared.
    for (const [n, u] of [['evil.example', 'https://evil.example/'], ['example.org', 'https://example.org/']]) {
      const r = await step('A.4 step outside ' + n, id, u);
      const form = r.errorCode === L2.MSG.OUTSIDE_CODE ? 'mapped' : r.errorMessage === L2.MSG.outside ? 'quarry-message' : 'other';
      check('A.4 ' + n + ' is 403 for the host', r.status === 403 && form !== 'other', st(r) + ' form=' + form);
    }
    // The spec author's expectation, not a plan sentence: if it fails, record a finding, do not weaken A.4.
    const a4b = await step('A.4 the session survives the refusals', id, 'https://example.com/');
    check('A.4 granted host still 200 afterwards', a4b.status === 200, st(a4b));

    // A.5 close (revoke) and a step on the closed session. Expected [CODE]: close 200 {closed:true};
    // the step then 404 'agent run not found' (run_action reads the owner projection first, :928-938)
    // or a typed 4xx naming a resume id. Not provable here: the broker refusing the revoked grant on a
    // LIVE run (spec A5c) - wrapper.
    const a5 = await closeSession('A.5 close', id);
    check('A.5 close is 200 closed:true', a5.status === 200 && data(a5).closed === true, st(a5));
    const a5b = await step('A.5 step after close', id, 'https://example.com/');
    check('A.5 a closed session answers 4xx, never 2xx or 5xx', a5b.status >= 400 && a5b.status < 500, st(a5b));
  });

  // ---------------------------------------------------------------- Leg A: A.6 expiry (section 3)
  // Start a fresh session, step once (200), wait T + 5 s, step again: the grant is past its TTL, the
  // broker answers inactive, Quarry answers 403 'browser grant is inactive or expired' (QE/agent_routes/
  // admission.rs:33-45, per-step revalidation), not a 5xx. Mirrors spec A5d. Then an AI run on the
  // expired session: 409 browser_session_grant_inactive (GW/domains/browser.rs:1835-1846 via
  // session_grant_inactive :4854-4861: MG 409 browser_grant_not_derivable, or MG 403 'browser grant is
  // not authorized' with no hosts named). If the AI-run answer is something else, record it; the
  // [CODE] line is then unconfirmed.
  // T = the broker's BROWSER_BROKER_GRANT_TTL_SECONDS from the preflight (45 on the contract stack).
  // Note: a first-host session's Quarry run carries max_runtime_s 120 (GW/domains/browser.rs:4634-4645),
  // so a T above ~110 s may meet the runtime cap first.
  L2.legA6 = (ttlSeconds) => runLeg('A6', async () => {
    const T = Number(ttlSeconds) > 0 ? Number(ttlSeconds) : 45;
    note('A.6 TTL used', T + ' s (wait ' + (T + 5) + ' s)');
    if (T + 5 > 110) note('A.6 wait vs run runtime cap', 'T + 5 > 110 s: the run max_runtime_s 120 may end the run before the grant expires; record what appears');
    const a6 = await startSession('A.6 start', { url: 'https://example.com' }, 'A6');
    L2.s.A6 = { id: a6.id };
    check('A.6 started', a6.r.status === 200 && !!a6.id, st(a6.r) + (a6.id ? ' id=' + a6.id : ''));
    if (!a6.id) { note('A.6 rest', 'no session id', 'NOT-RUN'); return; }
    const a6s = await step('A.6 step before expiry', a6.id, 'https://example.com/');
    check('A.6 step before expiry is 200', a6s.status === 200, st(a6s));
    const startedAt = Date.now();
    console.log('waiting', T + 5, 's');
    await sleep((T + 5) * 1000);
    const a6e = await step('A.6 step after expiry', a6.id, 'https://example.com/');
    check('A.6 403 for the grant, not a 5xx', a6e.status === 403 && a6e.errorMessage === L2.MSG.inactive,
      st(a6e) + ' waited=' + Math.round((Date.now() - startedAt) / 1000) + 's');
    const a6r = await startAiRun('A.6 AI run on an expired session', a6.id, { goal: 'Open example.com.' }, 'A6');
    check('A.6 AI run is 409 browser_session_grant_inactive', a6r.r.status === 409 && a6r.r.errorCode === 'browser_session_grant_inactive', st(a6r.r));
    if (a6r.run) await stopRun('A.6 cleanup stop unexpected run', a6r.run, { cleanup: true });
    await closeSession('A.6 cleanup close', a6.id, { cleanup: true });
  });

  // ---------------------------------------------------------------- Leg A: close-out
  // Safety net and leg-A verdict (section 3, "What to record" and "Leg A passes when"): closes every
  // session still open from any leg (close is idempotent), stops any AI run not stopped, then reports
  // whether every A check passed and no leg-A response was 5xx. The host-side scan (section 7) is the
  // other half of the verdict and stays with the lead.
  L2.legA_close = () => runLeg('Aclose', async () => {
    for (const run of Object.keys(L2.openRuns)) await stopRun('A.close stop run', run, { cleanup: true });
    for (const id of Object.keys(L2.open)) await closeSession('A.close close', id, { cleanup: true });
    check('A.close nothing left open', Object.keys(L2.open).length === 0 && Object.keys(L2.openRuns).length === 0,
      'sessions=' + Object.keys(L2.open).length + ' runs=' + Object.keys(L2.openRuns).length);
    // A checks: 'A.n ...' rows plus the leg-wide rows of legs A0, A, A6, A7 ('A0 no 401 ...', 'A no 5xx ...').
    const aChecks = results.filter((r) => /^A([0-9.]| )/.test(r.name) && r.ok === false);
    const a5xx = rows.filter((r) => /^A[.0-9]/.test(r.label) && r.status >= 500 && !r.cleanup);
    const ran = ['A0', 'A', 'A6'].filter((k) => L2.legs[k] && L2.legs[k].state === 'done');
    check('A verdict (page side): every A check PASS and no A response 5xx', aChecks.length === 0 && a5xx.length === 0 && ran.length === 3,
      'legs done=' + ran.join(',') + ' failed checks=' + aChecks.length + ' 5xx=' + a5xx.length);
  });

  // ---------------------------------------------------------------- Leg A.7 (optional, section 3)
  // Another user's session. Needs a SECOND signed-in account in this pane (never reuse the first
  // user's cookie). In the FIRST account: id = await L2.openForA7() (keep the id; it is not a secret).
  // Then sign in as the second account, inject this file again, run L2.legA7(id). Back in the first
  // account: L2.closeById(id). Expected [CODE]: 403 or 404 for both (Quarry owner check,
  // QE/agent_routes/lifecycle.rs; an unknown run for the caller is 'agent run not found').
  // The first account's run carries max_runtime_s 120: if more time passed, both answers are
  // trivially 404, so the detail records the age passed in, and the lead notes it.
  L2.openForA7 = async () => {
    const o = await startSession('A.7 prep start (first account)', { url: 'https://example.com' }, 'A7prep');
    return o.id || ('no id: ' + st(o.r));
  };
  L2.closeById = async (id) => st(await closeSession('close by id', id, { cleanup: true }));
  L2.legA7 = (sid, ageSeconds) => {
    if (!sid) {
      const why = 'no second account or no session id given (optional leg)';
      note('A.7 other user', why, 'NOT-RUN');
      return Promise.resolve([{ name: 'A.7 other user', ok: null, detail: 'NOT-RUN: ' + why }]);
    }
    return runLeg('A7', async () => {
      const a7 = await call('A.7 other user reads the session', 'GET', sessPath(sid));
      const a7b = await step('A.7 other user steps', sid, 'https://example.com/');
      check('A.7 neither is 2xx or 5xx', [a7.status, a7b.status].every((s) => s >= 400 && s < 500),
        st(a7) + ' | ' + st(a7b) + (ageSeconds != null ? ' | session age ' + ageSeconds + 's' : ''));
    });
  };

  // ---------------------------------------------------------------- Leg B (section 4)
  // SC1's "UI session starts, acts and closes with zero 401/403 and zero grant-less fallbacks", and
  // the strict default. Gate stack; B.1 only means something when the gateway is strict.
  L2.legB = () => runLeg('B', async () => {
    // B.1 a host the broker cannot grant (public IP literal; passes GW/public_url.rs:48-57; the broker
    // wants dotted non-IP hosts) is refused BEFORE any Quarry run. Expected [CODE]: 400
    // browser_host_not_grantable (GW/domains/browser.rs:4584-4591). Only that proves strict.
    // permissive on a production stack: 403 'browser grant_id is required' from Quarry; off: 200.
    const b1 = await startSession('B.1 non-grantable host', { url: 'https://1.1.1.1/' }, 'B');
    check('B.1 strict refuses: 400 browser_host_not_grantable', b1.r.status === 400 && b1.r.errorCode === 'browser_host_not_grantable', st(b1.r));
    note('B.1 outcome', b1.r.status === 400 && b1.r.errorCode === 'browser_host_not_grantable' ? 'strict'
      : b1.r.status === 200 ? 'NOT STRICT: session opened (off, permissive without grant, or human_browse)'
      : b1.r.status === 403 ? 'likely permissive on a production Quarry: ' + st(b1.r) : 'other: ' + st(b1.r));
    if (b1.id) await closeSession('B.1 cleanup', b1.id, { cleanup: true });
    L2.mark('B.happy.from');

    // B.2 start, tabs, close. Routes: /tabs GET (:483-486, get_tabs :1054). Expected [CODE]: 200, 200, 200.
    // Host-side: one broker 'browser grant issued' (kind session) and one 'browser grant revoked'.
    const b2 = await startSession('B.2 start', { url: 'https://example.com' }, 'B');
    check('B.2 200 with a session id and the first observation', b2.r.status === 200 && !!b2.id && !!b2.d.observation,
      st(b2.r) + (b2.id ? ' id=' + b2.id : ''));
    note('B.2 start ms (plan Risks: grant adds ~100-150 ms)', String(b2.r.ms));
    if (b2.id) {
      const b2t = await call('B.2 tabs', 'GET', sessPath(b2.id) + '/tabs');
      check('B.2 tabs 200', b2t.status === 200, st(b2t));
      const b2c = await closeSession('B.2 close', b2.id);
      check('B.2 close 200', b2c.status === 200, st(b2c));
    }

    // B.3 a ZDR session (memory-only z_ grant in the broker). Expected [CODE]: 200 with session.zdr true
    // (GW/domains/browser.rs:617, 3256), step 200. zdr with a persistent profile would be 400
    // zdr_persistent_profile_forbidden (not tested). Broker log line has zdr:true and no grant id.
    const b3 = await startSession('B.3 start zdr', { url: 'https://example.com', zdr: true }, 'B');
    check('B.3 200, session zdr:true', b3.r.status === 200 && !!b3.d.session && b3.d.session.zdr === true, st(b3.r) + (b3.id ? ' id=' + b3.id : ''));
    if (b3.id) {
      const b3s = await step('B.3 step', b3.id, 'https://example.com/');
      check('B.3 step 200', b3s.status === 200, st(b3s));
      const b3c = await closeSession('B.3 close', b3.id);
      check('B.3 close 200', b3c.status === 200, st(b3c));
    }
    L2.mark('B.happy.to');
  });

  // ---------------------------------------------------------------- Leg C (section 5)
  // User bearer -> gateway -> Model Gateway DeriveRunGrant -> execution-core -> Quarry: a run grant is
  // derived from the session grant, the run acts and observes, no refusal. On a stack with an LLM
  // planner this spends model tokens (maxSteps 3); record which planner ran.
  L2.legC = (pollSeconds) => runLeg('C', async () => {
    L2.mark('C.happy.from');
    const c1 = await startSession('C.1 start session', { url: 'https://example.com' }, 'C');
    L2.s.C = { session: c1.id };
    check('C.1 session started', c1.r.status === 200 && !!c1.id, st(c1.r) + (c1.id ? ' id=' + c1.id : ''));
    if (!c1.id) { note('C.2-C.5', 'no session id', 'NOT-RUN'); return; }

    // C.2 the AI run. Omit allowedDomains: the run grant derives from the session grant's single host.
    // Expected [CODE]: 200 {data:{run_id,...}}. Typed refusals: 409 browser_session_grant_missing
    // (:1750-1763), 409 browser_session_grant_inactive (:1835-1846), 503 delegated_auth_unavailable,
    // 400 invalid_browser_goal (:1709-1720). Any 401 or 403 here is a FAIL of leg C.
    const c2 = await startAiRun('C.2 start AI run', c1.id, { goal: 'Open example.com and read its title.', maxSteps: 3 }, 'C');
    L2.s.C.run = c2.run;
    check('C.2 200 with a run id', c2.r.status === 200 && !!c2.run, st(c2.r) + (c2.run ? ' run=' + c2.run : ''));
    note('C.2 start ms', String(c2.r.ms));

    // C.3 poll the durable replay until both event types appear. Route GET /api/v1/runs/{run_id}/events/
    // replay (GW/domains/chat.rs:77-80; streams.rs:226-273 passes Model Gateway's body through).
    // Code-confirmed shape (2026-10-29), different from the doc: top-level, no data envelope,
    // {run_id, events:[{event_id, event_type, at, payload}], next_event_id, truncated}
    // (MG/http_routes.rs:1255-1272, 1280-1376), and event types are the proto names
    // EVENT_TYPE_BROWSER_ACTION_DISPATCHED / EVENT_TYPE_BROWSER_OBSERVATION_RECEIVED (:1348-1356).
    // Both spellings are accepted. Only types and counts are kept; payloads are never printed.
    if (c2.run) {
      const canon = (t) => String(t || '').replace(/^EVENT_TYPE_/, '').replace(/_/g, '').toLowerCase();
      const DISPATCHED = 'browseractiondispatched', OBSERVED = 'browserobservationreceived';
      const seen = [];
      let last, cursor = null, refusal = false, polls = 0;
      const deadline = Date.now() + (Number(pollSeconds) > 0 ? Number(pollSeconds) : 75) * 1000;
      while (Date.now() < deadline) {
        polls++;
        const q = 'limit=200' + (cursor ? '&after_event_id=' + enc(cursor) : '');
        last = await call('C.3 replay', 'GET', '/api/v1/runs/' + enc(c2.run) + '/events/replay?' + q);
        if (last.status !== 200) break;
        const body = data(last);
        const evs = Array.isArray(body.events) ? body.events : [];
        for (const e of evs) {
          const t = e.eventType ?? e.event_type;
          seen.push(t);
          // The doc scans the whole events JSON; here only type + payload are scanned, so a
          // timestamp like '...31.403+00:00' or an event id cannot raise a false hit. On a hit the
          // event TYPE is recorded, never the payload.
          if (/\b40[13]\b|PermissionDenied|Unauthenticated|not authorized/.test(JSON.stringify([t, e.payload ?? null]))) refusal = refusal || t;
        }
        const next = body.nextEventId ?? body.next_event_id;
        if (next) cursor = next;
        if (seen.map(canon).includes(OBSERVED) && seen.map(canon).includes(DISPATCHED)) break;
        if (!(body.truncated === true)) await sleep(3000);
      }
      const kinds = seen.map(canon);
      check('C.3 replay readable', !!last && last.status === 200, last ? st(last) + ' polls=' + polls : 'no poll');
      check('C.3 BrowserActionDispatched seen', kinds.includes(DISPATCHED));
      check('C.3 BrowserObservationReceived seen', kinds.includes(OBSERVED));
      check('C.3 no 401/403/PermissionDenied/Unauthenticated in the run events', !refusal, refusal ? 'hit in an event of type ' + refusal : '');
      const counts = {};
      for (const t of seen) counts[t] = (counts[t] || 0) + 1;
      note('C.3 event types seen', Object.entries(counts).map(([k, v]) => k + 'x' + v).join(',') || 'none');
    } else {
      note('C.3', 'no run id from C.2', 'NOT-RUN');
    }
    L2.mark('C.happy.to');

    // C.4 widening negative on a fresh session (one run per session): naming a host the session grant
    // does not cover. Expected [CODE]: 403 {error:'browser grant is not authorized'} forwarded unchanged
    // because hosts were named (session_grant_inactive :4854-4861 only maps when no hosts are named;
    // MG/browser_run.rs:1011-1048). A 409 browser_grant_not_derivable would be the plan's alternative
    // mapping: it is recorded as FAIL with what appeared.
    const c4s = await startSession('C.4 start session', { url: 'https://example.com' }, 'C');
    L2.s.C.session2 = c4s.id;
    check('C.4 session started', c4s.r.status === 200 && !!c4s.id, st(c4s.r));
    if (c4s.id) {
      const c4 = await startAiRun('C.4 widening request', c4s.id, { goal: 'Open example.com.', allowedDomains: ['example.com', 'evil.example'] }, 'C');
      check('C.4 403 browser grant is not authorized', c4.r.status === 403 && c4.r.errorMessage === L2.MSG.widening, st(c4.r));
      if (c4.run) await stopRun('C.4 cleanup stop unexpected run', c4.run, { cleanup: true });
    }

    // C.5 stop and close. A 2xx or a typed 4xx for an already-finished run are both fine; a 5xx is
    // recorded (cleanup flag: not counted by the leg's no-5xx check).
    if (L2.s.C.run) {
      const c5 = await stopRun('C.5 stop run', L2.s.C.run, { cleanup: true });
      note('C.5 stop run', st(c5) + (c5.status >= 500 ? ' (5xx recorded)' : ''));
    }
    for (const sid of [L2.s.C.session, L2.s.C.session2]) {
      if (sid && L2.open[sid]) {
        const c5c = await closeSession('C.5 close', sid, { cleanup: true });
        note('C.5 close ' + sid, st(c5c));
      }
    }
  });

  // ---------------------------------------------------------------- summary (section 8 data)
  L2.summary = () => {
    const verdict = (r) => (r.ok === true ? 'PASS' : r.ok === false ? 'FAIL' : String(r.detail).startsWith('NOT-RUN') ? 'NOT-RUN' : 'INFO');
    const table = results.map((r) => ({ verdict: verdict(r), name: r.name, detail: r.detail }));
    const totals = table.reduce((a, r) => { a[r.verdict] = (a[r.verdict] || 0) + 1; return a; }, {});
    return {
      t0: L2.t0,
      marks: { ...L2.marks },
      legs: L2.legStatus(),
      totals,
      openSessions: Object.keys(L2.open),
      openRuns: Object.keys(L2.openRuns),
      responses5xx: rows.filter((r) => r.status >= 500).map((r) => r.label + ' ' + r.status),
      responses401: rows.filter((r) => r.status === 401).map((r) => r.label),
      results: table,
      requests: rows.map(({ label, method, status, errorCode, errorMessage, ms, transport, at }) =>
        ({ at, label, method, status, errorCode, errorMessage: clip(errorMessage), ms, transport })),
    };
  };

  console.log('L2 ready, t0 =', L2.t0);
  return 'L2 ready, t0=' + L2.t0 + (prev ? ' (re-injected; ' + results.length + ' results kept)' : '') +
    '. Legs: legA0, legA_start, legA6(ttl), legA_close, legB, legC, legA7(sid); then summary()';
})();
