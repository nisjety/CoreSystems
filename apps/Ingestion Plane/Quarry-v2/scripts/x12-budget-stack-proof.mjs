#!/usr/bin/env node
// x12-budget-stack-proof.mjs: the scripted stack proof for the agent-run budget
// (plan WS6.B deployment evidence, decision X-12, ratified 2026-10-28).
//
// It is for the stack session. The build does not run it, and it must not be run
// against the live stack. It SETS NOTHING: no flag, no env file, no compose change.
// It only starts agent runs on the edge you point it at and drives one past each
// bound, printing PASS, FAIL or NOT-RUN per bound with the value it read.
//
// What the stack must already have (the stack session sets these, not this script):
//   QUARRY_EDGE__BUDGET_ENFORCEMENT=on
//   QUARRY_EDGE__AGENT_DEFAULT_MAX_STEPS=5      (a short run; the shipped default is 60)
// Everything else is the shipped default, so the bounds below are the ratified numbers:
// 900 s and 1 USD by default, ceilings 300 steps, 3600 s and 20 USD.
//
// A fresh run has no page, and on the chromium driver every page action (the default wait
// included) fails with no_current_page until something navigates. So the first step of every
// run this script drives is a navigate to PAGE_URL. It is a real step: it is counted in each
// bound below, and it costs 0.001 USD on the flat table (crates/quarry-runtime/src/action_cost.rs:
// navigate 0.001 + bytes x 1e-7, the edge projects and charges it with 0 bytes; wait 0.0001).
//
// Bounds proved (each one is its own run):
//   ceilings   a request above the ceiling is clamped to 300 / 3600 s / 20 USD, by "ceiling"
//   steps      an agent run asking for nothing stops at the default: 402 BUDGET_EXHAUSTED,
//              dimension steps, exactly that many 200s before it (the navigate and the rest),
//              and exactly that many receipts, each with cost_micro_usd > 0 and cost_source
//              flat_table
//   runtime    max_runtime_s = RUNTIME_S: after it passes the next step is 402, dimension runtime
//   cost       max_cost_usd = one navigate + three actions on the flat table (0.0013 USD with the
//              default wait): the navigate and at least one action are 200, then 402 dimension
//              cost, refused before it runs: no 200's flat-table part (spent_usd minus the
//              browser-second term at its elapsed_s) passes the limit, one more action would pass
//              the refusal's spent figure, and the refused calls leave no receipt. The
//              browser-second term (0.00003 USD per active second) keeps growing after the gate,
//              so a full spent figure may pass the limit by a second or two of it, and it can take
//              the third action's room on a slow stack: the number of 200s is not fixed.
//   people     a person's own steps are never capped: PERSON_STEPS (default 20) steps, all 200,
//              on a session whose request cap is 12 (the gateway's number; the pre-slice
//              baseline answered 429 on the 13th)
//   guard      inside that kind of session a model-proposed step (initiated_by agent) keeps the
//              request cap: the step past it is 429, not 402, and the person is still not capped
//   takeover   a person's steps while holding an agent-class run's browser are exempt: well past
//              a 2-step bound, all 200, and the ledger (meta.budget.steps) does not move; after the
//              hand-back the person's step counts again and the next one is 402
//
// Inputs come from the environment. No secret goes on the command line or into this file,
// and no token is ever printed.
//   QUARRY_TOKEN        a Quarry user token (an org member), e.g. from /api/quarry/token
// Optional:
//   EDGE_URL            the edge (default http://127.0.0.1:8082, the Ingestion compose port)
//   GRANT_ID            a browser grant id, when the stack requires grants for every action
//   RUNTIME_S           the runtime bound to prove (default 5)
//   PERSON_STEPS        how many person steps to drive (default 20)
//   PAGE_URL            the page the first step of each run opens (default
//                       http://fixture-a.crawl.test/, the crawl fixture on the Ingestion stack)
//   ACTION_JSON         the action to drive after it (default {"type":"wait","ms":0}; the cost
//                       bound is sized from its flat-table price)
//   BUDGET_BROWSER_SECOND_USD  the edge's QUARRY_EDGE__BUDGET_BROWSER_SECOND_USD, when it is
//                       not the shipped 0.00003 (the cost bound takes it out of spent_usd)
//   STEP_TIMEOUT_MS     per-request timeout (default 60000)
//
// Usage:
//   node scripts/x12-budget-stack-proof.mjs               # the proof (stack session only)
//   node scripts/x12-budget-stack-proof.mjs --self-test   # judge canned replies; no network
//
// Exit code: 0 when every bound is PASS, 1 when any is FAIL, 2 when a precondition
// (token, enforcement on) is not met and nothing was proved.
//
// Not covered here, and recorded by hand in the evidence file (plan WS6.B step 4 and 5):
//   - an idle-expired run resumed with resume_run_id is refused on its first step;
//   - psql on quarry_agent_run_checkpoints shows state->'budget'->>'exhausted' = 'steps';
//   - SSE (GET /v1/agent/runs/{id}/events/stream) shows exactly one agent.budget_exhausted;
//   - until the Model Plane maps the 402, execution-core reports the run failed with a step error.

const RESULTS = [];

function record(name, status, detail) {
  RESULTS.push({ name, status, detail });
  console.log(`${status.padEnd(7)} ${name.padEnd(9)} ${detail}`);
}

// ── judgments (pure; --self-test runs these on canned replies) ──────────────

/** A 402 BUDGET_EXHAUSTED naming `dimension`, with the limit when one is given. */
export function judgeRefusal(reply, dimension, limit) {
  if (reply.status !== 402) return `status ${reply.status}, want 402 (${brief(reply)})`;
  const error = reply.body?.error ?? {};
  if (error.code !== 'BUDGET_EXHAUSTED') return `code ${error.code}, want BUDGET_EXHAUSTED`;
  if (error.retryable !== false) return `retryable ${error.retryable}, want false`;
  const details = error.details ?? {};
  if (details.dimension !== dimension) return `dimension ${details.dimension}, want ${dimension}`;
  if (limit !== undefined && details.limit !== limit) return `limit ${details.limit}, want ${limit}`;
  return null;
}

/** Receipts of a stopped run: `count` of them, each with a cost and the flat table as source. */
export function judgeReceipts(receipts, count) {
  if (!Array.isArray(receipts)) return 'receipts is not a list';
  if (receipts.length !== count) return `${receipts.length} receipts, want ${count}`;
  const free = receipts.findIndex((receipt) => !(receipt.cost_micro_usd > 0));
  if (free >= 0) return `receipt ${free + 1} has cost_micro_usd ${receipts[free].cost_micro_usd}`;
  const other = receipts.findIndex((receipt) => receipt.cost_source !== 'flat_table');
  if (other >= 0) return `receipt ${other + 1} has cost_source ${receipts[other].cost_source}`;
  return null;
}

/** The ceilings of the start reply: 300 steps, 3600 s, 20 USD, each clamped by "ceiling". */
export function judgeCeilings(reply) {
  const effective = reply.body?.data?.constraints ?? {};
  const clamps = reply.body?.data?.clamped ?? [];
  const want = [
    ['max_steps', 300],
    ['max_runtime_s', 3600],
    ['max_cost_usd', 20],
  ];
  for (const [field, value] of want) {
    if (effective[field] !== value) return `${field} ${effective[field]}, want ${value}`;
    const clamp = clamps.find((entry) => entry.field === field);
    if (!clamp || clamp.by !== 'ceiling') return `${field} not reported as clamped by ceiling`;
  }
  return null;
}

/**
 * The flat-table price of an action in micro-USD, as the edge projects it: the table in
 * crates/quarry-runtime/src/action_cost.rs with 0 bytes (estimated_action_cost_usd in
 * quarry-edge agent_routes/budget.rs). Wire types not named here cost 0.0001.
 */
export function flatMicroUsd(action) {
  switch (action?.type) {
    case 'navigate':
    case 'evaluate':
      return 1000;
    case 'screenshot':
    case 'pdf':
      return 5000;
    default:
      return 100;
  }
}

/** The cost bound to ask for: the opening navigate plus three of `action`, in USD. */
export function costLimitUsd(action) {
  return (flatMicroUsd({ type: 'navigate' }) + 3 * flatMicroUsd(action)) / 1e6;
}

const micro = (usd) => Math.round(Number(usd) * 1e6);

/**
 * The flat-table part of a 200's meta.budget.spent_usd, in micro-USD: the spent figure minus
 * the browser-second term (whole active seconds x the rate), both read at the same instant.
 * The gate admits an action when spent so far + its flat price fits, but the browser-second
 * term keeps growing after it, so the full figure of a correctly admitted step can pass the
 * limit by a browser second or two. The flat part cannot.
 */
export function flatPartMicro(step, browserSecondMicro) {
  const budget = step.body?.meta?.budget ?? {};
  if (typeof budget.spent_usd !== 'number' || typeof budget.elapsed_s !== 'number') return null;
  return micro(budget.spent_usd) - Math.floor(budget.elapsed_s) * browserSecondMicro;
}

/**
 * The cost run: `steps` are the replies that came before the first refusal (the navigate
 * first), `refusal` is that refusal, `again` the call after it, and `receipts` the run's
 * receipts read after both. The refusal must be made on the projection, before the action
 * runs: no admitted step's flat-table charges pass the limit, the refusal comes only when one
 * more action would pass it, and the two refused calls leave no receipt.
 */
export function judgeCost(steps, refusal, again, receipts, limit, action, browserSecondMicro) {
  if (steps.length < 2) {
    const statuses = steps.map((step) => step.status).join(', ') || 'none';
    return `only ${steps.length} steps answered 200 before the stop (${statuses}), want the navigate and at least one action`;
  }
  for (const [index, step] of steps.entries()) {
    const flat = flatPartMicro(step, browserSecondMicro);
    if (flat === null) return `step ${index + 1} has no meta.budget.spent_usd and elapsed_s`;
    if (flat > micro(limit)) return `step ${index + 1} charged ${flat / 1e6} USD on the flat table, over the ${limit} limit`;
  }
  if (refusal === null) return 'never refused';
  const problem = judgeRefusal(refusal, 'cost', limit);
  if (problem) return problem;
  const spent = refusal.body.error.details.spent;
  if (typeof spent !== 'number') return 'the refusal names no spent figure';
  if (micro(spent) + flatMicroUsd(action) <= micro(limit)) {
    return `refused at ${spent} USD although one more action (${flatMicroUsd(action) / 1e6}) fits under ${limit}`;
  }
  const latched = judgeRefusal(again, 'cost', limit);
  if (latched) return `the call after the refusal: ${latched}`;
  if (typeof again.body.error.details.spent !== 'number') return 'the call after the refusal names no spent figure';
  // Neither refused call ran: each admitted step has its receipt, a refused one has none.
  if (!Array.isArray(receipts)) return 'the receipts are not a list';
  if (receipts.length !== steps.length) {
    return `${receipts.length} receipts for ${steps.length} admitted steps (a refused call ran)`;
  }
  const charged = receipts.reduce((sum, receipt) => sum + (Number(receipt.cost_micro_usd) || 0), 0);
  if (charged > micro(limit)) return `the receipts charge ${charged / 1e6} USD, over the ${limit} limit`;
  return null;
}

function brief(reply) {
  const text = JSON.stringify(reply.body ?? null);
  return text.length > 200 ? `${text.slice(0, 200)}...` : text;
}

// ── the edge, over HTTP ─────────────────────────────────────────────────────

const edgeUrl = (process.env.EDGE_URL ?? 'http://127.0.0.1:8082').replace(/\/+$/, '');
const timeoutMs = Number(process.env.STEP_TIMEOUT_MS ?? 60000);
const action = process.env.ACTION_JSON ? JSON.parse(process.env.ACTION_JSON) : { type: 'wait', ms: 0 };
const browserSecondMicro = micro(process.env.BUDGET_BROWSER_SECOND_USD ?? 0.00003);
// The first step of every driven run: a page for the actions after it (see the header).
const openPage = { type: 'navigate', url: process.env.PAGE_URL ?? 'http://fixture-a.crawl.test/' };

async function call(token, method, route, body) {
  const response = await fetch(`${edgeUrl}${route}`, {
    method,
    headers: {
      authorization: `Bearer ${token}`,
      ...(body === undefined ? {} : { 'content-type': 'application/json' }),
    },
    body: body === undefined ? undefined : JSON.stringify(body),
    signal: AbortSignal.timeout(timeoutMs),
  });
  const text = await response.text();
  let parsed = null;
  try {
    parsed = text ? JSON.parse(text) : null;
  } catch {
    parsed = { unparsable: text.slice(0, 200) };
  }
  return { status: response.status, body: parsed };
}

// The wire type of constraints needs max_steps, with 0 meaning "not asked": it is filled in when a
// bound other than steps is named, and the whole object is left out when nothing is asked.
function startBody(extra) {
  const body = { ...extra };
  if (body.constraints && Object.keys(body.constraints).length === 0) delete body.constraints;
  else if (body.constraints) body.constraints = { max_steps: 0, ...body.constraints };
  if (process.env.GRANT_ID) body.grant_id = process.env.GRANT_ID;
  return body;
}

async function startRun(token, extra) {
  const reply = await call(token, 'POST', '/v1/agent/runs', startBody(extra));
  const runId = reply.body?.data?.run_id;
  return { reply, runId };
}

const stepOf = (token, runId, hint, what = action) =>
  call(token, 'POST', `/v1/agent/runs/${runId}/step`, {
    action: what,
    ...(hint ? { initiated_by: hint } : {}),
  });

/** The run's opening step: the navigate to PAGE_URL, counted like any other step. */
const openOf = (token, runId, hint) => stepOf(token, runId, hint, openPage);

const closeRun = (token, runId) => call(token, 'DELETE', `/v1/agent/runs/${runId}`).catch(() => null);

async function control(token, runId, mode) {
  return call(token, 'POST', `/v1/agent/runs/${runId}/browser-session/control`, { mode });
}

// ── the bounds ──────────────────────────────────────────────────────────────

async function proveCeilings(token) {
  const { reply, runId } = await startRun(token, {
    initiated_by: 'agent',
    constraints: { max_steps: 1000, max_runtime_s: 100000, max_cost_usd: 100 },
  });
  if (!runId) return record('ceilings', 'FAIL', `start refused: ${brief(reply)}`);
  const problem = judgeCeilings(reply);
  await closeRun(token, runId);
  record('ceilings', problem ? 'FAIL' : 'PASS', problem ?? 'clamped to 300 steps, 3600 s, 20 USD by "ceiling"');
}

async function proveSteps(token) {
  const { reply, runId } = await startRun(token, { initiated_by: 'agent', constraints: {} });
  if (!runId) return record('steps', 'FAIL', `start refused: ${brief(reply)}`);
  const limit = reply.body?.data?.constraints?.max_steps;
  if (!Number.isInteger(limit) || limit > 100) {
    await closeRun(token, runId);
    return record(
      'steps',
      'NOT-RUN',
      `the default is ${limit} steps; start the edge with QUARRY_EDGE__AGENT_DEFAULT_MAX_STEPS=5 for a short proof`,
    );
  }
  let done = 0;
  let refusal = null;
  for (let n = 0; n <= limit + 1; n += 1) {
    const step = n === 0 ? await openOf(token, runId, undefined) : await stepOf(token, runId, undefined);
    if (step.status === 200) {
      done += 1;
    } else {
      refusal = step;
      break;
    }
  }
  const receipts = await call(token, 'GET', `/v1/agent/runs/${runId}/receipts`);
  await closeRun(token, runId);
  const problem =
    done !== limit
      ? `${done} steps answered 200 before the stop, want ${limit}`
      : refusal === null
        ? 'never refused'
        : (judgeRefusal(refusal, 'steps', limit) ?? judgeReceipts(receipts.body?.data, limit));
  record('steps', problem ? 'FAIL' : 'PASS', problem ?? `${limit} steps, then 402 steps; ${limit} receipts with a flat-table cost`);
}

async function proveRuntime(token) {
  const seconds = Number(process.env.RUNTIME_S ?? 5);
  const { reply, runId } = await startRun(token, {
    initiated_by: 'agent',
    constraints: { max_runtime_s: seconds },
  });
  if (!runId) return record('runtime', 'FAIL', `start refused: ${brief(reply)}`);
  const first = await openOf(token, runId, undefined);
  await new Promise((resolve) => setTimeout(resolve, (seconds + 1.5) * 1000));
  const late = await stepOf(token, runId, undefined);
  await closeRun(token, runId);
  const problem =
    first.status !== 200 ? `the first step was ${first.status} (${brief(first)})` : judgeRefusal(late, 'runtime', seconds);
  record('runtime', problem ? 'FAIL' : 'PASS', problem ?? `402 runtime after ${seconds} s`);
}

async function proveCost(token) {
  const limit = costLimitUsd(action);
  const { reply, runId } = await startRun(token, {
    initiated_by: 'agent',
    constraints: { max_cost_usd: limit },
  });
  if (!runId) return record('cost', 'FAIL', `start refused: ${brief(reply)}`);
  // The navigate, then actions until the first refusal (at most five; three fit at best).
  const steps = [];
  let refusal = null;
  for (let n = 0; n <= 5 && refusal === null; n += 1) {
    const step = n === 0 ? await openOf(token, runId, undefined) : await stepOf(token, runId, undefined);
    if (step.status === 200) steps.push(step);
    else refusal = step;
  }
  const again = refusal === null ? null : await stepOf(token, runId, undefined);
  const receipts = await call(token, 'GET', `/v1/agent/runs/${runId}/receipts`);
  await closeRun(token, runId);
  const problem = judgeCost(steps, refusal, again, receipts.body?.data, limit, action, browserSecondMicro);
  record(
    'cost',
    problem ? 'FAIL' : 'PASS',
    problem ??
      `${steps.length} steps within ${limit} USD on the flat table, then 402 cost at ${refusal.body.error.details.spent} USD, before it ran`,
  );
}

async function provePeople(token) {
  const count = Number(process.env.PERSON_STEPS ?? 20);
  const { reply, runId } = await startRun(token, { constraints: { max_steps: 12 } });
  if (!runId) return record('people', 'FAIL', `start refused: ${brief(reply)}`);
  let bad = null;
  for (let n = 1; n <= count && bad === null; n += 1) {
    const step = n === 1 ? await openOf(token, runId, undefined) : await stepOf(token, runId, undefined);
    if (step.status !== 200) bad = `person step ${n} was ${step.status} (${brief(step)})`;
  }
  await closeRun(token, runId);
  record('people', bad ? 'FAIL' : 'PASS', bad ?? `${count} person steps on a 12-step session, all 200`);
}

async function proveGuard(token) {
  const { reply, runId } = await startRun(token, { constraints: { max_steps: 3 } });
  if (!runId) return record('guard', 'FAIL', `start refused: ${brief(reply)}`);
  // The model opens the page itself, so the request cap still sees three model steps.
  const agent = [];
  for (let n = 0; n < 3; n += 1) {
    agent.push((n === 0 ? await openOf(token, runId, 'agent') : await stepOf(token, runId, 'agent')).status);
  }
  const past = await stepOf(token, runId, 'agent');
  const person = await stepOf(token, runId, undefined);
  await closeRun(token, runId);
  const problem = agent.some((status) => status !== 200)
    ? `the model's first three steps were ${agent.join(', ')}`
    : past.status !== 429
      ? `the model's step past the cap was ${past.status}, want 429 (${brief(past)})`
      : past.body?.error?.code === 'BUDGET_EXHAUSTED'
        ? 'the guard answered with the typed 402, want the legacy 429'
        : person.status !== 200
          ? `the person's step on the same session was ${person.status}`
          : null;
  record('guard', problem ? 'FAIL' : 'PASS', problem ?? "the model's 4th step is 429, the person's step is 200");
}

async function proveTakeover(token) {
  const { reply, runId } = await startRun(token, { initiated_by: 'agent', constraints: { max_steps: 2 } });
  if (!runId) return record('takeover', 'FAIL', `start refused: ${brief(reply)}`);
  const counted = await openOf(token, runId, undefined);
  const took = await control(token, runId, 'human_takeover');
  if (took.status !== 200) {
    await closeRun(token, runId);
    return record('takeover', 'FAIL', `the takeover was refused: ${brief(took)}`);
  }
  const during = [];
  let ledger = null;
  for (let n = 0; n < 5; n += 1) {
    const step = await stepOf(token, runId, undefined);
    during.push(step.status);
    ledger = step.body?.meta?.budget?.steps ?? ledger;
  }
  await control(token, runId, 'agent_control');
  const second = await stepOf(token, runId, undefined);
  const refused = await stepOf(token, runId, undefined);
  await closeRun(token, runId);
  const problem =
    counted.status !== 200
      ? `the step before the takeover was ${counted.status}`
      : during.some((status) => status !== 200)
        ? `takeover steps were ${during.join(', ')}, want all 200`
        : ledger !== null && ledger !== 1
          ? `meta.budget.steps was ${ledger} during the takeover, want 1 (the exempt steps are not counted)`
          : second.status !== 200
            ? `the first step after the hand-back was ${second.status}`
            : judgeRefusal(refused, 'steps', 2);
  record('takeover', problem ? 'FAIL' : 'PASS', problem ?? '5 takeover steps exempt; counted again after the hand-back; then 402 steps');
}

// ── self-test: the judgments on canned replies, no network ──────────────────

function selfTest() {
  let failures = 0;
  const expect = (label, got, want) => {
    const ok = want === null ? got === null : typeof got === 'string' && got.includes(want);
    if (!ok) {
      failures += 1;
      console.log(`SELF-TEST FAIL ${label}: got ${JSON.stringify(got)}, want ${JSON.stringify(want)}`);
    }
  };
  const refusal = (dimension, limit, extra = {}) => ({
    status: 402,
    body: { error: { code: 'BUDGET_EXHAUSTED', retryable: false, details: { dimension, limit }, ...extra } },
  });
  expect('refusal ok', judgeRefusal(refusal('steps', 5), 'steps', 5), null);
  expect('refusal wrong dimension', judgeRefusal(refusal('cost', 5), 'steps', 5), 'dimension cost');
  expect('refusal wrong limit', judgeRefusal(refusal('steps', 6), 'steps', 5), 'limit 6');
  expect('refusal is a 429', judgeRefusal({ status: 429, body: {} }, 'steps', 5), 'want 402');
  expect('refusal retryable', judgeRefusal(refusal('steps', 5, { retryable: true }), 'steps', 5), 'retryable');
  const receipt = { cost_micro_usd: 100, cost_source: 'flat_table' };
  expect('receipts ok', judgeReceipts([receipt, receipt], 2), null);
  expect('receipts short', judgeReceipts([receipt], 2), '1 receipts');
  expect('receipts free', judgeReceipts([receipt, { ...receipt, cost_micro_usd: 0 }], 2), 'receipt 2');
  expect('receipts source', judgeReceipts([{ ...receipt, cost_source: 'estimated' }], 1), 'cost_source');
  const clamps = ['max_steps', 'max_runtime_s', 'max_cost_usd'].map((field) => ({ field, by: 'ceiling' }));
  const ceilings = (constraints, clamped = clamps) => ({ status: 200, body: { data: { constraints, clamped } } });
  expect('ceilings ok', judgeCeilings(ceilings({ max_steps: 300, max_runtime_s: 3600, max_cost_usd: 20 })), null);
  expect('ceilings steps', judgeCeilings(ceilings({ max_steps: 60, max_runtime_s: 3600, max_cost_usd: 20 })), 'max_steps 60');
  expect('ceilings unreported', judgeCeilings(ceilings({ max_steps: 300, max_runtime_s: 3600, max_cost_usd: 20 }, [])), 'not reported');

  // The cost bound, sized from the flat table: navigate 0.001 and three waits at 0.0001.
  const wait = { type: 'wait', ms: 0 };
  const sized = costLimitUsd(wait) === 0.0013 ? null : `costLimitUsd(wait) is ${costLimitUsd(wait)}`;
  expect('cost limit for wait', sized, null);
  const navigateSized = costLimitUsd({ type: 'navigate', url: 'x' }) === 0.004 ? null : 'navigate limit';
  expect('cost limit for navigate', navigateSized, null);
  const ok = (spent, elapsed) => ({ status: 200, body: { meta: { budget: { spent_usd: spent, elapsed_s: elapsed } } } });
  const costRefusal = (spent) => refusal('cost', 0.0013, { details: { dimension: 'cost', limit: 0.0013, spent } });
  const paid = (...prices) => prices.map((cost_micro_usd) => ({ cost_micro_usd, cost_source: 'flat_table' }));
  const judge = (steps, first, second, receipts) => judgeCost(steps, first, second, receipts, 0.0013, wait, 30);
  // The by-hand stack run of 2026-10-28 (X12.5e): 0.001, 0.0011, 0.00123 (one browser second in it),
  // then 402 at 0.00123 twice.
  const stack = [ok(0.001, 0.727), ok(0.0011, 0.912), ok(0.00123, 1.096)];
  const stackPaid = paid(1000, 100, 100);
  expect('cost stack run', judge(stack, costRefusal(0.00123), costRefusal(0.00123), stackPaid), null);
  expect('cost one browser second later', judge(stack, costRefusal(0.00123), costRefusal(0.00126), stackPaid), null);
  // A faster navigate: the third wait is admitted at 0.0012 + 0.0001 before the first second ends,
  // and its meta (and both refusals) read after it carry a browser second over the limit. Correct.
  const fast = [ok(0.001, 0.5), ok(0.0011, 0.7), ok(0.0012, 0.9), ok(0.00133, 1.05)];
  const fastPaid = paid(1000, 100, 100, 100);
  expect('cost fast stack', judge(fast, costRefusal(0.00133), costRefusal(0.00133), fastPaid), null);
  expect('cost slow stack, one wait', judge(stack.slice(0, 2), costRefusal(0.00122), costRefusal(0.00122), paid(1000, 100)), null);
  expect('cost navigate refused', judge([], costRefusal(0), costRefusal(0), []), 'only 0 steps');
  expect('cost no action fits', judge(stack.slice(0, 1), costRefusal(0.001), costRefusal(0.001), paid(1000)), 'only 1 steps');
  expect('cost early refusal', judge(stack.slice(0, 2), costRefusal(0.0011), costRefusal(0.0011), paid(1000, 100)), 'fits under');
  const over = [...fast, ok(0.00143, 1.2)];
  expect('cost overshoot', judge(over, costRefusal(0.00143), costRefusal(0.00143), paid(1000, 100, 100, 100, 100)), 'over the');
  expect('cost refused call ran', judge(stack, costRefusal(0.00123), costRefusal(0.00133), paid(1000, 100, 100, 100)), 'a refused call ran');
  expect('cost receipts over', judge(stack, costRefusal(0.00123), costRefusal(0.00123), paid(1000, 100, 300)), 'receipts charge');
  expect('cost no receipts', judge(stack, costRefusal(0.00123), costRefusal(0.00123), undefined), 'not a list');
  const unpriced = refusal('cost', 0.0013, { details: { dimension: 'cost', limit: 0.0013 } });
  expect('cost again unpriced', judge(stack, costRefusal(0.00123), unpriced, stackPaid), 'names no spent');
  expect('cost never refused', judge(stack, null, null, stackPaid), 'never refused');
  expect('cost wrong dimension', judge(stack, refusal('steps', 5), refusal('steps', 5), stackPaid), 'dimension steps');
  const bare = [ok(0.001, 0.5), { status: 200, body: {} }];
  expect('cost no meta', judge(bare, costRefusal(0.0012), costRefusal(0.0012), paid(1000, 100)), 'no meta.budget');
  console.log(failures === 0 ? 'SELF-TEST PASS' : `SELF-TEST FAIL (${failures})`);
  return failures === 0 ? 0 : 1;
}

// ── main ────────────────────────────────────────────────────────────────────

async function main() {
  if (process.argv.includes('--self-test')) process.exit(selfTest());
  const token = process.env.QUARRY_TOKEN;
  if (!token) {
    console.error('PRECONDITION QUARRY_TOKEN is not set (a Quarry user token of an org member)');
    process.exit(2);
  }
  // Which mode the edge is in, from the edge itself.
  const probe = await startRun(token, { initiated_by: 'agent', constraints: {} });
  if (!probe.runId) {
    console.error(`PRECONDITION the edge refused a plain agent start: ${brief(probe.reply)}`);
    process.exit(2);
  }
  const mode = probe.reply.body?.data?.constraints?.enforcement;
  await closeRun(token, probe.runId);
  if (mode !== 'on') {
    console.error(
      `PRECONDITION enforcement is ${JSON.stringify(mode)}, not "on". This script sets nothing: ` +
        'the stack session sets QUARRY_EDGE__BUDGET_ENFORCEMENT=on and restarts the edge first. Nothing was proved.',
    );
    process.exit(2);
  }
  console.log(`edge ${edgeUrl}, enforcement on`);

  const bounds = [proveCeilings, proveSteps, proveRuntime, proveCost, provePeople, proveGuard, proveTakeover];
  for (const bound of bounds) {
    try {
      await bound(token);
    } catch (error) {
      record(bound.name.replace(/^prove/, '').toLowerCase(), 'FAIL', `threw: ${error?.message ?? error}`);
    }
  }
  const failed = RESULTS.filter((result) => result.status === 'FAIL').length;
  const notRun = RESULTS.filter((result) => result.status === 'NOT-RUN').length;
  console.log(`${RESULTS.length - failed - notRun} PASS, ${failed} FAIL, ${notRun} NOT-RUN`);
  process.exit(failed > 0 ? 1 : notRun > 0 ? 1 : 0);
}

await main();
