// Docs contract for the web-rerank ownership decision (Quarry option B).
//
// The Data Plane owns reranking of caller-supplied candidates (matrix rule 15).
// That decision lives in five documents, and a document that drifts from the
// decision is exactly how a false claim ships. This test pins what each one must
// say, reading the real files, so a later edit cannot quietly undo it:
//
//   apps/master-ownership-matrix.md                  rows 70-71, rule 2, rule 15, rule 9 and rule 18
//   apps/CODEBASE_INFORMATION_SYSTEM.md              rule 2, retrieval-engine-rs
//   apps/GDPR_SUMMARY.md                             Data Plane processor-gate note
//   apps/Model Plane/docs/decisions/ledger.md        the rerank decision entry and the
//                                                    service-principal exception entry
//   apps/Frontend Plane/verevonv3/docs/WEB_SOURCE_QUALITY_AUDIT_2026-09-15.md   A3 status
//
// The second half of the file (QB-T13b) pins rule 18, the bounded exception to
// rule 9 under which the retrieval engine admits a service principal by a
// default-deny route allowlist. Unlike the rest, it also reads the SOURCE the
// documents describe (the engine's route table and refusal codes, the gRPC
// refusal, documents-api-go's service-scope middleware) so a doc that names a
// route, scope or code the code does not have, or a code table the docs do not
// list, fails.
//
// Run:  node --test scripts/tests/ownership-docs-contract.test.mjs
//
// It never skips: a missing file is a failure, because a docs contract that
// silently passes when its subject is absent is not a contract.
//
// It deliberately asserts nothing about rules 16 and 17 (other docs tasks own
// them) beyond "the numbering stays contiguous", and nothing about line numbers.

import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync, readdirSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), '..', '..');

const FILES = {
  matrix: 'apps/master-ownership-matrix.md',
  cis: 'apps/CODEBASE_INFORMATION_SYSTEM.md',
  gdpr: 'apps/GDPR_SUMMARY.md',
  ledger: 'apps/Model Plane/docs/decisions/ledger.md',
  audit: 'apps/Frontend Plane/verevonv3/docs/WEB_SOURCE_QUALITY_AUDIT_2026-09-15.md',
};

function load(key) {
  const path = resolve(ROOT, FILES[key]);
  const buf = readFileSync(path); // throws (fails the test) when absent
  const raw = buf.toString('utf8');
  return { path, buf, raw, text: raw.replace(/\r\n/g, '\n') };
}

// The code the rule 18 documents describe. Read for real, never mocked.
const SOURCES = {
  routeTable: 'apps/Data Plane v2/services/retrieval-engine-rs/src/authz/service_routes.rs',
  admission: 'apps/Data Plane v2/services/retrieval-engine-rs/src/authz/principal.rs',
  grpc: 'apps/Data Plane v2/services/retrieval-engine-rs/src/grpc/interceptor.rs',
  documentsMiddleware: 'apps/Data Plane v2/services/documents-api-go/pkg/authctx/authctx.go',
  documentsVerify: 'apps/Data Plane v2/services/documents-api-go/pkg/authctx/verify.go',
};

function loadSource(key) {
  // readFileSync throws (fails the test) when the source is absent: no skip.
  return readFileSync(resolve(ROOT, SOURCES[key]), 'utf8').replace(/\r\n/g, '\n');
}

/** Every `.rs` file under `dir`, recursively (absolute paths). Throws when `dir` is absent. */
function rustSourcesUnder(dir) {
  const files = [];
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    const path = resolve(dir, entry.name);
    if (entry.isDirectory()) files.push(...rustSourcesUnder(path));
    else if (entry.name.endsWith('.rs')) files.push(path);
  }
  return files;
}

/** Text between the first line matching `startRe` and the next line matching `endRe`. */
function section(text, startRe, endRe) {
  const lines = text.split('\n');
  const start = lines.findIndex((l) => startRe.test(l));
  assert.notEqual(start, -1, `section start not found: ${startRe}`);
  let end = lines.length;
  for (let i = start + 1; i < lines.length; i++) {
    if (endRe.test(lines[i])) {
      end = i;
      break;
    }
  }
  return lines.slice(start, end).join('\n');
}

/** Prose is hard-wrapped at about 80 columns: match phrases across line breaks and blockquote marks. */
function flat(text) {
  return text.replace(/^>\s?/gm, '').replace(/\s+/g, ' ');
}

function cellsOf(line) {
  return line
    .trim()
    .replace(/^\|/, '')
    .replace(/\|$/, '')
    .split('|')
    .map((c) => c.trim());
}

// ---------------------------------------------------------------- every file

for (const key of Object.keys(FILES)) {
  test(`${key}: no NUL bytes, uniform line endings, final newline`, () => {
    const { buf, raw } = load(key);
    assert.equal(buf.includes(0), false, `${FILES[key]} contains NUL bytes`);
    const crlf = (raw.match(/\r\n/g) || []).length;
    const bareLf = (raw.match(/(?<!\r)\n/g) || []).length;
    assert.ok(
      crlf === 0 || bareLf === 0,
      `${FILES[key]} mixes line endings (CRLF=${crlf}, bare LF=${bareLf})`,
    );
    assert.equal(/\r(?!\n)/.test(raw), false, `${FILES[key]} has a stray carriage return`);
    assert.ok(raw.endsWith('\n'), `${FILES[key]} must end with a newline`);
  });
}

// -------------------------------------------------------------------- matrix

test('matrix: every table row in section 1 still has exactly 8 cells', () => {
  const { text } = load('matrix');
  const table = section(text, /^## 1\. Canonical Plane Matrix/, /^## 2\./)
    .split('\n')
    .filter((l) => l.startsWith('|'));
  assert.ok(table.length > 30, 'section 1 table not found');
  for (const row of table) {
    assert.equal(cellsOf(row).length, 8, `broken table row (stray pipe?): ${row.slice(0, 90)}`);
  }
});

test('matrix: the vector retrieval / rerank row names the caller-supplied rerank ownership', () => {
  const { text } = load('matrix');
  const row = text.split('\n').find((l) => l.startsWith('| Vector retrieval / rerank / source join'));
  assert.ok(row, 'row "Vector retrieval / rerank / source join" not found');
  const [, quarry, dataPlane, modelPlane] = cellsOf(row);
  assert.match(quarry, /caller only/i, 'Quarry must be a caller, not "no"');
  assert.match(quarry, /POST \/v1\/rerank/, 'Quarry cell must name POST /v1/rerank');
  assert.match(quarry, /never reranks itself/i);
  assert.match(dataPlane, /Owner/);
  assert.match(dataPlane, /caller-supplied/i, 'Data Plane owns caller-supplied candidates too');
  assert.match(modelPlane, /never reranks itself/i);
});

test('matrix: the hybrid retrieval row no longer says Quarry has no part in it', () => {
  const { text } = load('matrix');
  const row = text.split('\n').find((l) => l.startsWith('| Hybrid retrieval (BM25 + dense + rerank)'));
  assert.ok(row, 'row "Hybrid retrieval (BM25 + dense + rerank)" not found');
  const [, quarry, dataPlane] = cellsOf(row);
  assert.notEqual(quarry.toLowerCase(), 'no');
  assert.match(quarry, /caller only/i);
  assert.match(dataPlane, /Owner/);
  // Quarry does own a Tantivy index over the pages it scraped itself
  // (quarry-runtime/src/local_index.rs): "owns no index" would be a false claim.
  assert.match(quarry, /local page index/i, 'Quarry cell must name its own local page index');
  assert.doesNotMatch(quarry, /owns no index\b/i);
  assert.match(quarry, /owns no tenant knowledge index/i);
});

test('matrix: rule 2 keeps its lab-only carve-out and points to rule 15', () => {
  const { text } = load('matrix');
  const rule = text.split('\n').find((l) => /^2\. \*\*/.test(l));
  assert.ok(rule, 'rule 2 not found');
  assert.match(rule, /never embed or rerank independently except in isolated eval labs/);
  assert.match(rule, /rule 15/);
  assert.match(rule, /never a model prompt of their own/);
});

test('matrix: rule 15 states the ownership, the gates and that it is not an availability claim', () => {
  const { text } = load('matrix');
  const rule = text.split('\n').find((l) => /^15\. \*\*/.test(l));
  assert.ok(rule, 'rule 15 not found in section 3');
  for (const needle of [
    /Rerank of caller-supplied candidates is Data Plane-owned/,
    /public web (search )?results/,
    /POST \/v1\/rerank/,
    /retrieval-engine-rs/,
    /Quarry and Model Plane are callers/,
    /never prompt a general-purpose model/,
    /paid/,
    /never runs on a subscription or bring-your-own route/,
    /by design/,
    /not a bypass of rule 2/,
    /run-level ZDR/,
    /explicit `skipped` marker/,
    /never a silent reorder/,
    /sovereign/,
    /processor gate/,
    /named service principal/,
    /Ownership, not availability/,
    /decision ledger/,
  ]) {
    assert.match(rule, needle, `rule 15 must contain ${needle}`);
  }
  // A claim of current availability would contradict the code until the live proof.
  assert.doesNotMatch(rule, /\b(is live|is deployed|currently serves|already serves)\b/i);
});

test('matrix: section 3 rule numbers are contiguous and reach at least 15', () => {
  const { text } = load('matrix');
  const numbers = section(text, /^## 3\. Non-negotiable Cross-Plane Contracts/, /^## 4\./)
    .split('\n')
    .map((l) => /^(\d+)\. \*\*/.exec(l))
    .filter(Boolean)
    .map((m) => Number(m[1]));
  assert.ok(numbers.length >= 15, `expected at least 15 rules, found ${numbers.length}`);
  numbers.forEach((n, i) => assert.equal(n, i + 1, `rule numbering breaks at position ${i + 1}`));
});

// ------------------------------------------------- rule 18 and rule 9 (QB-T13b)

function ruleLine(n) {
  const { text } = load('matrix');
  const rule = text.split('\n').find((l) => new RegExp(`^${n}\\. \\*\\*`).test(l));
  assert.ok(rule, `rule ${n} not found in section 3`);
  return rule;
}

test('matrix: rules 15 to 18 are all written, in plan order (15 rerank, 16 CR, 17 ZR, 18 QB)', () => {
  const { text } = load('matrix');
  const numbers = section(text, /^## 3\. Non-negotiable Cross-Plane Contracts/, /^## 4\./)
    .split('\n')
    .map((l) => /^(\d+)\. \*\*/.exec(l))
    .filter(Boolean)
    .map((m) => Number(m[1]));
  for (const n of [15, 16, 17, 18]) assert.ok(numbers.includes(n), `rule ${n} is missing`);
  assert.match(ruleLine(15), /^15\. \*\*Rerank of caller-supplied candidates/);
  assert.match(ruleLine(16), /^16\. \*\*Background content processing/);
  assert.match(ruleLine(17), /^17\. \*\*A user's own connection/);
  assert.match(ruleLine(18), /^18\. \*\*A first-party service principal/);
});

test('matrix: rule 18 states the service-principal exception to rule 9 and what stays Control-owned', () => {
  const rule = ruleLine(18);
  for (const needle of [
    /admitted to the Data Plane retrieval engine by a default-deny route allowlist, not by organisation membership/,
    /bounded exception to rule 9/,
    /`principal_type=service`/,
    /`not_member`/,
    /`retrieval-engine-rs`/,
    /exact method and route/,
    /scope its token must carry/,
    /403 `service_principal_route_not_allowed`/,
    /`service_principal_scope_missing`/,
    /matched route pattern, never the raw path/,
    /closed to every service until someone lists it/,
    // What stays Control's.
    /Identity and tenancy/,
    /signed by Auth Core/,
    /never a blanket "any organisation" in production/,
    /refuses a contradictory identity with 401/,
    // The engine refuses a DIFFERING header and, when there is none, takes the token's
    // organisation (service_principal_auth.rs: "an absent header uses the claim"), so the
    // rule must not say the header is required.
    /refuses an `x-org-id` header that differs from the token's organisation/,
    /pins the request to the token's organisation/,
    // What a service never gets.
    /No membership authority/,
    /`data:read`, `data:rerank`/,
    /dropped at admission/,
    /no per-user grants/,
    /visibility facade is never asked about it/,
    /`POST \/v1\/rerank`, rule 15/,
    /refuses a user before any membership lookup/,
    /gRPC surface, which authorises by membership, refuses a service outright/,
    /interactive user path is unchanged/,
    // Precedent and the terms for everyone else.
    /`documents-api-go` already admits a verified service principal/,
    /`RequireServiceScope`/,
    // A requirement on the others, not a claim about them: the ledger records that only the
    // retrieval engine and documents-api-go were read.
    /must do so on the same terms/,
    /No other service has been audited against this/,
    /never a header or a body field/,
    // Honest limits.
    /Not decided here/,
    /Control Plane principal work/,
    /Ownership, not availability/,
    /decision ledger/,
  ]) {
    assert.match(rule, needle, `rule 18 must contain ${needle}`);
  }
  // A claim of current availability would contradict the code until the live proof.
  assert.doesNotMatch(rule, /\b(is live|is deployed|currently serves|already serves)\b/i);
  // Two sentences the code does not support: the header is not required, and other
  // Data Plane services have not been shown to meet the rule.
  assert.doesNotMatch(rule, /requires the `x-org-id` header/);
  assert.doesNotMatch(rule, /Any other Data Plane service that admits a service principal does so/);
  // The rows live in the ledger, where the parity test below holds them to the code.
  assert.doesNotMatch(rule, /`\/v1\/retrieve`/, 'rule 18 states the rule; the ledger lists the rows');
});

test('matrix: rule 9 keeps its text and names rule 18 as its bounded exception', () => {
  const rule = ruleLine(9);
  assert.match(rule, /Identity and membership are Control Plane-owned/);
  assert.match(rule, /active-organization headers, session hints, and Application projections are not grants/);
  assert.match(rule, /Bounded exception by rule 18/);
  assert.match(rule, /default-deny route allowlist/);
  assert.match(rule, /interactive users are unchanged/);
});

test('rule 18 vs the code: every refusal, scope and middleware the docs name exists in the source', () => {
  const rule = ruleLine(18);
  const admission = loadSource('admission');
  for (const code of ['service_principal_route_not_allowed', 'service_principal_scope_missing', 'service_only_route']) {
    assert.ok(admission.includes(`"${code}"`), `${code} is named in the docs but is not a refusal code in principal.rs`);
  }
  // A contradictory identity is 401, and the user path still resolves membership.
  assert.match(admission, /StatusCode::UNAUTHORIZED,\s*"denied:principal_identity_invalid"/);
  assert.match(admission, /policy\.resolve\(/);

  // gRPC refuses a service in the interceptor and again in the handler's own check,
  // not by the membership lookup failing. Each refusal is read inside its own block.
  const grpc = loadSource('grpc');
  const refusal = /Status::permission_denied\(\s*"service principals are not accepted on this interface",?\s*\)/;
  const interceptorArm = /Ok\(Principal::Service \{ \.\. \}\) => \{([\s\S]*?)\n\s*\}\n\s*Err\(_\)/.exec(grpc);
  assert.ok(interceptorArm, 'the interceptor must classify a service principal explicitly');
  assert.match(interceptorArm[1], refusal, 'the interceptor must refuse a service with PermissionDenied');
  const handlerCheck = /if ctx\.principal\.is_service\(\) \{([\s\S]*?)\n\s*\}/.exec(grpc);
  assert.ok(handlerCheck, 'the handler authorisation must check ctx.principal.is_service()');
  assert.match(handlerCheck[1], refusal, 'the handler check must refuse a service with PermissionDenied');

  // The scopes a service context keeps are exactly the two the rule names, and the
  // organisation-wide visibility scopes are not among them.
  const routes = loadSource('routeTable');
  const consts = new Map([...routes.matchAll(/pub const (SCOPE_[A-Z_]+): &str = "([^"]+)";/g)].map((m) => [m[1], m[2]]));
  const ctx = /pub const SERVICE_CONTEXT_SCOPES: &\[&str\] = &\[([^\]]*)\];/.exec(routes);
  assert.ok(ctx, 'SERVICE_CONTEXT_SCOPES not found in service_routes.rs');
  const kept = [...ctx[1].matchAll(/SCOPE_[A-Z_]+/g)].map((m) => consts.get(m[0]));
  assert.deepEqual([...kept].sort(), ['data:read', 'data:rerank']);
  for (const scope of kept) assert.ok(rule.includes(`\`${scope}\``), `rule 18 must name the scope ${scope}`);
  assert.equal(kept.includes('org:data:read_all'), false);
  assert.equal(kept.includes('org:data:write_all'), false);

  // The precedent: documents-api-go admits a verified service on a route scope and
  // accepts a service identity only in the shape the docs describe.
  const middleware = loadSource('documentsMiddleware');
  assert.match(middleware, /func RequireServiceScope\(scope string\)/);
  assert.match(middleware, /claims\.IsService\(\) && claims\.HasScope\(scope\)/);
  const verify = loadSource('documentsVerify');
  assert.match(
    verify,
    /case "service":\s*identityValid = claims\.ServiceID != "" && claims\.UserID == "" && claims\.ServiceID == claims\.Subject && len\(claims\.Scopes\) > 0/,
  );
});

// ------------------------------------------------- CODEBASE_INFORMATION_SYSTEM

test('CODEBASE_INFORMATION_SYSTEM: rule 2 and the retrieval-engine entry carry the carve-out', () => {
  const { text } = load('cis');
  const rules = section(text, /^## Non-Negotiable Cross-Plane Rules/, /^## Plane Details/);
  const rule2 = rules.split('\n').find((l) => /^2\. /.test(l));
  assert.ok(rule2, 'rule 2 not found');
  assert.match(rule2, /No independent embeddings or reranking outside isolated labs/);
  assert.match(rule2, /reranking of tenant knowledge and of caller-supplied candidates/);
  assert.match(rule2, /rule 15/);
  assert.match(rule2, /ZDR, sovereign and processor gates/);

  const bullet = text.split('\n').find((l) => l.startsWith('- `retrieval-engine-rs`'));
  assert.ok(bullet, 'retrieval-engine-rs bullet not found');
  assert.match(bullet, /caller-supplied candidates/);
  assert.match(bullet, /rule 15/);
});

// --------------------------------------------------------------- GDPR_SUMMARY

test('GDPR_SUMMARY: the Data Plane note requires both processor conditions and records the gaps', () => {
  const { text } = load('gdpr');
  const dp = section(text, /^### Data Plane v2/, /^### Ingestion Plane/);
  const noteStart = dp.indexOf('Note: rerank of caller-supplied candidates');
  assert.notEqual(noteStart, -1, 'the rerank note is missing from the Data Plane v2 section');
  const note = flat(dp.slice(noteStart));
  for (const needle of [
    /by design/,
    /not a bypass of the Model Plane preference/,
    /processor is approved \*\*and\*\* `allow_third_party_processing=true`/,
    /Neither alone is enough/,
    /PrivacyPolicy\.allow_third_party_processing/,
    /not carried on (Quarry's|the) search path/,
    /retrieval and embedding engines read no such field/,
    /operator affirmation/,
    /open gap/,
    /sovereign/,
    /explicit marker/,
    /no evidence/i,
    /region/,
  ]) {
    assert.match(note, needle, `GDPR note must contain ${needle}`);
  }
  // The original controls it annotates stay in place.
  assert.match(flat(dp), /Deny direct external embedding, reranking, or enrichment calls unless the/);
  assert.match(flat(dp), /Prefer Model Plane provider policy gates/);
});

// --------------------------------------------------------------------- ledger

const LEDGER_HEADER = /^## 2026-10-03 — Web-search rerank: the Data Plane owns reranking of caller-supplied candidates \(Quarry option B\)$/m;

function ledgerEntry() {
  const { text } = load('ledger');
  const m = LEDGER_HEADER.exec(text);
  assert.ok(m, 'ledger entry for the rerank decision not found');
  const rest = text.slice(m.index);
  const next = rest.indexOf('\n---\n');
  assert.notEqual(next, -1, 'the entry must end with a --- separator');
  return { text, start: m.index, body: flat(rest.slice(0, next)) };
}

test('ledger: the rerank decision is the newest-first entry above the 2026-09-03 one', () => {
  const { text, start } = ledgerEntry();
  const older = text.indexOf('## 2026-09-03 — Quarry-v2 web search: ZDR did not gate Brave/Serper');
  assert.ok(older > start, 'the new entry must sit above the 2026-09-03 entry (newest first)');
  // Append-only: the entries below are untouched in their headers.
  assert.ok(text.includes('## 2026-08-28 — Data Plane retrieval: what to send when nothing declares a sovereignty posture'));
});

test('ledger: the entry has a valid state and records decision, evidence and limits', () => {
  const { body } = ledgerEntry();
  assert.match(body, /\*\*State:\*\* `(proposed|implemented)`/);
  for (const needle of [
    /option B/i,
    /POST \/v1\/rerank/,
    /paid/,
    /never a subscription/i,
    /Auth Core/,
    /403/,
    /models:invoke/,
    /keeping original order/,
    /text_rerank_allowed/,
    /skipped/,
    /input order/,
    /allow_third_party_processing/,
    /sovereign-rag-blueprint-reconciliation/,
    /sovereign-rag-phased-plan/,
    /Deliberately not done/,
    /gRPC/,
    /self-hosted/,
    /Where it lands/,
    /Trigger/,
    /zdr_mode/,
    /documents\.source/,
    /rrf_fuse/,
    // Owner default D-C: the other Model Plane dependents are tracked, not forgotten.
    /DEF-QB-T17-PART2/,
    /owner default D-C/i,
  ]) {
    assert.match(body, needle, `ledger entry must contain ${needle}`);
  }
  // Nothing in this entry may claim a build or a deploy that has not been proven.
  assert.doesNotMatch(body, /\b(is deployed|has been deployed|is live in production)\b/i);
});

test('ledger: every file the rerank capability and the Quarry client touch is referenced', () => {
  const { body } = ledgerEntry();
  // Data Plane rerank capability (QB-T05a) and Quarry's client (QB-T09).
  for (const tail of [
    'retrieval-engine-rs/src/rerank_api/',
    'retrieval-engine-rs/src/search/rerank.rs',
    'retrieval-engine-rs/src/rate_limit/mod.rs',
    'retrieval-engine-rs/src/audit/mod.rs',
    'retrieval-engine-rs/src/config.rs',
    'retrieval-engine-rs/src/main.rs',
    'retrieval-engine-rs/src/lib.rs',
    'retrieval-engine-rs/src/api/mod.rs',
    'retrieval-engine-rs/src/metrics/mod.rs',
    'retrieval-engine-rs/tests/rerank_contract.rs',
    'retrieval-engine-rs/tests/fixtures/contracts/rerank-v1/',
    'quarry-runtime/src/rerank.rs',
    'quarry-runtime/src/data_plane_http.rs',
    'quarry-runtime/src/lib.rs',
    'quarry-edge/src/main.rs',
    'quarry-edge/src/config.rs',
    'quarry-edge/src/search_routes.rs',
    'quarry-edge/tests/rerank_behavior.rs',
    'quarry-runtime/tests/dp_rerank_contract.rs',
    'quarry-runtime/tests/fixtures/dp-contracts/rerank-v1/',
  ]) {
    assert.ok(body.includes(tail), `ledger entry must reference ${tail}`);
  }
});

// ----------------------------------- ledger: the rule 9 exception entry (QB-T13b)

const SP_LEDGER_HEADER =
  /^## \d{4}-\d{2}-\d{2} — Service principals reach the retrieval engine by a route allowlist, not by membership: a bounded exception to matrix rule 9$/m;

function serviceEntry() {
  const { text } = load('ledger');
  const m = SP_LEDGER_HEADER.exec(text);
  assert.ok(m, 'ledger entry for the service-principal exception to matrix rule 9 not found');
  const rest = text.slice(m.index);
  const next = rest.indexOf('\n---\n');
  assert.notEqual(next, -1, 'the entry must end with a --- separator');
  const raw = rest.slice(0, next);
  return { text, start: m.index, raw, body: flat(raw) };
}

/** The rows of SERVICE_ROUTE_GRANTS, read from the engine's own source. */
function routeTableFromCode() {
  const src = loadSource('routeTable');
  const scopes = new Map([...src.matchAll(/pub const (SCOPE_[A-Z_]+): &str = "([^"]+)";/g)].map((m) => [m[1], m[2]]));
  const block = /pub const SERVICE_ROUTE_GRANTS: &\[ServiceRouteGrant\] = &\[([\s\S]*?)\n\];/.exec(src);
  assert.ok(block, 'SERVICE_ROUTE_GRANTS not found in service_routes.rs');
  const rows = [
    ...block[1].matchAll(/grant\(\s*"([^"]+)",\s*"([^"]+)",\s*"([^"]+)",\s*(SCOPE_[A-Z_]+),?\s*\)/g),
  ].map((m) => {
    assert.ok(scopes.has(m[4]), `unknown scope constant ${m[4]}`);
    return `${m[1]} ${m[2]} ${m[3]} ${scopes.get(m[4])}`;
  });
  const calls = (block[1].match(/\bgrant\(/g) || []).length;
  assert.equal(rows.length, calls, 'a grant(..) row in service_routes.rs could not be parsed');
  assert.ok(rows.length > 0, 'the route table is empty');
  return rows.sort();
}

test('ledger: the service-principal exception is newest-first, above the rerank entry it extends', () => {
  const { text, start } = serviceEntry();
  const rerank = LEDGER_HEADER.exec(text);
  assert.ok(rerank, 'the rerank entry must still exist');
  assert.ok(start < rerank.index, 'the new entry must sit above the 2026-10-03 rerank entry (newest first)');
  // Append-only: the entries below keep their headers.
  assert.ok(text.includes('## 2026-09-03 — Quarry-v2 web search: ZDR did not gate Brave/Serper'));
});

test('ledger: the service-principal entry records decision, bounds, precedent, limits and trigger', () => {
  const { body } = serviceEntry();
  assert.match(body, /\*\*State:\*\* `(proposed|implemented)`/);
  for (const needle of [
    /matrix rule 9/,
    /matrix rule 18/,
    /`not_member`/,
    /default-deny/,
    /Not chosen: a membership row per service/,
    /authz\/service_routes\.rs/,
    /tests\/service_principal_auth\.rs/,
    /real rules/,
    // The bounds.
    /Tenancy stays Control's/,
    /`allowAnyOrg`/,
    /`x-org-id`/,
    /A contradictory identity is 401/,
    /`org:data:read_all`/,
    /visibility facade/,
    /`service_principal_route_not_allowed`/,
    /`service_principal_scope_missing`/,
    /matched route pattern/,
    /`POST \/v1\/rerank`, rule 15/,
    /gRPC stays membership-gated/,
    /`PermissionDenied`/,
    // The precedent and where it differs.
    /`documents-api-go`/,
    /authctx\.RequireServiceScope/,
    /does not name which service/,
    // What is flagged, not decided.
    /Deliberately not done/,
    /`data:rerank` grant/,
    /were not audited/,
    // How it flips.
    /Trigger to flip to `implemented`/,
    /`dpv2_authz_denials_total`/,
  ]) {
    assert.match(body, needle, `ledger entry must contain ${needle}`);
  }
  // Nothing in this entry may claim a build or a deploy that has not been proven.
  assert.doesNotMatch(body, /\b(is deployed|has been deployed|is live in production)\b/i);
});

test('ledger: the precedent comparison is exact about where documents-api-go names a service', () => {
  const { body } = serviceEntry();
  // documents-api-go admits a route on its scope alone (RequireServiceScope), but its
  // optional Space-import decision envelope does name one service. The entry says both,
  // so "it never names a service" is not left standing.
  const spaceImport = readFileSync(
    resolve(ROOT, 'apps/Data Plane v2/services/documents-api-go/internal/handler/space_import.go'),
    'utf8',
  );
  assert.match(spaceImport, /claims\.ServiceID != "imports-core"/, 'the named-service check the ledger describes is gone');
  assert.match(body, /does not name which service may call a route/);
  assert.match(body, /`imports-core`/);
  assert.match(body, /Space-import decision envelope, not route admission/);
});

test('ledger: the service-principal entry says whether the rerank route is registered, as the code has it', () => {
  const { body } = serviceEntry();
  // The route is registered when any engine source other than the allowlist itself names
  // "/v1/rerank" as a quoted path. The scan covers the whole src tree, not api/mod.rs
  // alone: the rerank capability is planned as its own sub-router module (rerank_api/)
  // merged into the router, so a check of api/mod.rs would stay green after the route
  // went live. The Cohere provider URL ("https://api.cohere.ai/v1/rerank") does not match
  // because the quote must sit directly before the slash. The pending state is the one this
  // entry was written in: the table row and the service-only refusal exist, the handler
  // (rule 15's capability task) does not.
  const srcDir = resolve(ROOT, 'apps/Data Plane v2/services/retrieval-engine-rs/src');
  const allowlist = resolve(srcDir, 'authz/service_routes.rs');
  const registeredIn = rustSourcesUnder(srcDir).filter(
    (file) => file !== allowlist && /"\/v1\/rerank"/.test(readFileSync(file, 'utf8')),
  );
  assert.ok(rustSourcesUnder(srcDir).includes(allowlist), 'the engine source tree was not found: the scan would pass vacuously');
  const registered = registeredIn.length > 0;
  if (registered) {
    assert.doesNotMatch(
      body,
      /not registered in the engine's router yet/,
      `POST /v1/rerank is now registered in the router (${registeredIn.join(', ')}): reconcile the ledger entry (rerank row, service-only refusal and live-proof step 5)`,
    );
  } else {
    assert.match(
      body,
      /not registered in the engine's router yet/,
      'the router does not register POST /v1/rerank yet: the entry must say so instead of implying the route answers',
    );
  }
});

test('ledger: the live-proof cost claims match what the engine does for ZDR and sovereignty', () => {
  const { body } = serviceEntry();
  // The code: ephemeral ZDR restricts cloud egress AND skips the trace; sovereign_required
  // alone restricts cloud egress but the trace is still persisted.
  const orchestrator = readFileSync(
    resolve(ROOT, 'apps/Data Plane v2/services/retrieval-engine-rs/src/pipeline/orchestrator.rs'),
    'utf8',
  ).replace(/\r\n/g, '\n');
  assert.match(orchestrator, /let embed_zdr = embed_zdr_for_mode\(zdr_mode\);/);
  assert.match(orchestrator, /let cloud_egress_restricted = embed_zdr \|\| sovereign_required;/);
  assert.match(orchestrator, /let trace_id = if zdr_mode\.is_ephemeral\(\) \{/);
  // The entry: the free proof uses ephemeral, and it does not credit sovereignty with the trace skip.
  assert.match(body, /`zdr_mode` `ephemeral`/);
  assert.match(body, /buys no query embedding and persists no trace/);
  assert.match(body, /`sovereign_required` true alone [^.]*still persists a trace/);
  assert.doesNotMatch(
    body,
    /`sovereign_required` true, restricts egress, so no embedding is bought and no trace is persisted/,
    'sovereign_required alone does not skip the trace',
  );
  // Every refusal the proof expects is a code the engine emits.
  const admission = loadSource('admission');
  for (const code of ['service_principal_route_not_allowed', 'service_only_route']) {
    assert.ok(body.includes(`\`${code}\``), `the live proof names ${code}`);
    assert.ok(admission.includes(`"${code}"`), `${code} is not emitted by principal.rs`);
  }
});

test('ledger: the service-principal entry lists exactly the rows of the engine route table', () => {
  const { raw } = serviceEntry();
  const documented = [...raw.matchAll(/^- `(service:[^`]+)` `([A-Z]+)` `(\/[^`]+)` needs `([^`]+)`$/gm)]
    .map((m) => `${m[1]} ${m[2]} ${m[3]} ${m[4]}`)
    .sort();
  assert.ok(documented.length > 0, 'the entry lists no rows in the form: - `service:x` `POST` `/v1/route` needs `scope`');
  assert.deepEqual(
    documented,
    routeTableFromCode(),
    'the ledger rows and SERVICE_ROUTE_GRANTS in service_routes.rs have drifted: change both together',
  );
});

test('ledger: every refusal code the service-principal entry names is a code the engine emits', () => {
  const { body } = serviceEntry();
  const admission = loadSource('admission');
  const named = new Set([...body.matchAll(/`(service_[a-z_]+)`/g)].map((m) => m[1]));
  assert.ok(named.has('service_principal_route_not_allowed'));
  for (const code of named) {
    assert.ok(admission.includes(`"${code}"`), `the entry names ${code}, which principal.rs does not emit`);
  }
});

// ---------------------------------------------------------------------- audit

test('audit: A3 has a dated status that says what actually happened', () => {
  const { text } = load('audit');
  const status = flat(section(text, /^### A3 status — updated 2026-10-03/, /^## 0\. One-screen verdict/));
  for (const needle of [
    /932b30d2/,
    /never (been )?effective|has not once changed an order/,
    /Auth Core/,
    /403/,
    /models:invoke/,
    /keeping original order/,
    /not judged/,
    /matrix rule 15/,
    /decision ledger/,
    /autoprompt/i,
  ]) {
    assert.match(status, needle, `A3 status must contain ${needle}`);
  }
});

test('audit: W-05 no longer claims a clean pass for a reranker that never ran', () => {
  const { text } = load('audit');
  const row = text.split('\n').find((l) => l.startsWith('| W-05 ranking = engine order'));
  assert.ok(row, 'W-05 row not found');
  const cells = cellsOf(row);
  assert.notEqual(cells[1], '✅', 'W-05 status must not be a bare pass');
  assert.match(row, /A3 status/);
});
