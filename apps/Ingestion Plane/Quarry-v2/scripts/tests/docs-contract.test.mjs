// Docs and contract checks for Quarry option B (the Data Plane rerank) and the Model Plane switches.
//
// Run from the repository root:
//   node --test "apps/Ingestion Plane/Quarry-v2/scripts/tests/docs-contract.test.mjs"
//
// The checks read the real files: the Ingestion compose (the quarry-edge block), quarry-edge's config
// and main, the Quarry docs and openapi, the cross-plane token-request manifest that the code loads,
// and the committed Control Plane registry. A missing file fails the test; nothing is skipped.

import { test } from "node:test";
import assert from "node:assert/strict";
import { existsSync, readdirSync, readFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import { parseYamlSubset } from "../yaml-subset.mjs";

const here = dirname(fileURLToPath(import.meta.url));
const quarry = resolve(here, "..", "..");
const ingestion = resolve(quarry, "..");
const apps = resolve(ingestion, "..");

const paths = {
  compose: join(ingestion, "docker-compose.yml"),
  config: join(quarry, "crates", "quarry-edge", "src", "config.rs"),
  main: join(quarry, "crates", "quarry-edge", "src", "main.rs"),
  docs: join(quarry, "docs"),
  readme: join(quarry, "README.md"),
  openapi: join(quarry, "docs", "openapi.yaml"),
  crossPlaneAuth: join(quarry, "docs", "CROSS_PLANE_AUTH.md"),
  crossPlaneIntegration: join(quarry, "docs", "CROSS_PLANE_INTEGRATION.md"),
  runbooks: join(quarry, "docs", "RUNBOOKS.md"),
  selfHost: join(quarry, "docs", "SELF_HOST.md"),
  tokenRequests: join(quarry, "crates", "quarry-runtime", "contracts", "cross-plane-token-requests.json"),
  registry: join(apps, "Control Plane", "config", "plane-service-principals.json"),
};

const read = (path) => {
  assert.ok(existsSync(path), `required file is present: ${path}`);
  return readFileSync(path, "utf8");
};

// Every Quarry document that a reader is told to trust: the docs folder and the README.
const currentDocs = () => {
  const files = readdirSync(paths.docs)
    .filter((name) => name.endsWith(".md"))
    .map((name) => join(paths.docs, name));
  files.push(paths.readme);
  return files.map((path) => ({ path, text: read(path) }));
};

// The quarry-edge service block of the Ingestion compose: the lines under `  quarry-edge:` up to the next
// service key (two spaces, a name, a colon).
function quarryEdgeBlock(compose) {
  const lines = compose.split(/\r?\n/);
  const start = lines.findIndex((line) => line === "  quarry-edge:");
  assert.notEqual(start, -1, "the compose has a quarry-edge service");
  let end = lines.length;
  for (let i = start + 1; i < lines.length; i++) {
    if (/^  [A-Za-z0-9_.-]+:\s*$/.test(lines[i])) {
      end = i;
      break;
    }
  }
  return lines.slice(start + 1, end);
}

// The environment of the block: keys at six spaces, values as written (quotes removed).
function environmentOf(block) {
  const env = new Map();
  for (const line of block) {
    const match = /^ {6}([A-Z][A-Z0-9_]*): (.*)$/.exec(line);
    if (match) env.set(match[1], match[2].replace(/^"(.*)"$/, "$1"));
  }
  return env;
}

// The default a compose interpolation writes: `${NAME:-default}` gives `default`; a literal gives itself.
function composeDefault(value) {
  const match = /\$\{[A-Z0-9_]+:-([^}]*)\}/.exec(value);
  return match ? match[1] : value;
}

// The properties of one component schema, read by indentation: the schema sits at four spaces under
// `  schemas:`, its properties at eight spaces under `      properties:`. Each property maps to its raw body.
function schemaProperties(spec, name) {
  const lines = spec.split(/\r?\n/);
  const schemasAt = lines.indexOf("  schemas:");
  assert.notEqual(schemasAt, -1, "the openapi has components.schemas");
  const start = lines.findIndex((line, index) => index > schemasAt && line === `    ${name}:`);
  assert.notEqual(start, -1, `the openapi defines schema ${name}`);
  let end = lines.length;
  for (let i = start + 1; i < lines.length; i++) {
    if (/^ {4}\S/.test(lines[i]) || /^\S/.test(lines[i])) {
      end = i;
      break;
    }
  }
  const block = lines.slice(start + 1, end);
  const properties = new Map();
  const at = block.findIndex((line) => line === "      properties:");
  if (at === -1) return properties;
  let current = null;
  for (let i = at + 1; i < block.length; i++) {
    const line = block[i];
    if (/^ {6}\S/.test(line)) break;
    const match = /^ {8}([A-Za-z_][A-Za-z0-9_]*):/.exec(line);
    if (match) {
      current = match[1];
      properties.set(current, []);
    } else if (current && line.trim() !== "") {
      properties.get(current).push(line);
    }
  }
  return properties;
}

// The body of one path (for example `/v1/search`) under `paths:`, up to the next path key.
function pathBlock(spec, route) {
  const lines = spec.split(/\r?\n/);
  const start = lines.findIndex((line) => line === `  ${route}:`);
  assert.notEqual(start, -1, `the openapi defines ${route}`);
  let end = lines.length;
  for (let i = start + 1; i < lines.length; i++) {
    if (/^ {2}\S/.test(lines[i]) || /^\S/.test(lines[i])) {
      end = i;
      break;
    }
  }
  return lines.slice(start + 1, end).join("\n");
}

// The one line of a Markdown table row that names a route, for example `quarry-edge → data-plane`.
function rowLine(text, label) {
  const line = text.split(/\r?\n/).find((candidate) => candidate.includes(label));
  assert.ok(line, `the Markdown row for "${label}" is present`);
  return line;
}

const tokenRows = JSON.parse(read(paths.tokenRequests)).rows;
const registry = JSON.parse(read(paths.registry));

// ----------------------------------------------------------------------------- removed identifiers

test("no removed rerank identifier remains in the compose, the config, the code or the docs", () => {
  const removed = [
    "SEMANTIC_RERANK_MODEL",
    "semantic_rerank_model",
    "ModelPlaneSearchReranker",
    "build_rerank_prompt",
    "reranker can call Model Plane",
    "RERANK_WINDOW",
  ];
  const sources = [
    ["compose", read(paths.compose)],
    ["config.rs", read(paths.config)],
    ["main.rs", read(paths.main)],
    ["openapi.yaml", read(paths.openapi)],
    ...currentDocs().map(({ path, text }) => [path, text]),
  ];
  for (const [label, text] of sources) {
    for (const identifier of removed) {
      assert.ok(!text.includes(identifier), `${label} still names ${identifier}`);
    }
  }
});

test("no current document says the semantic reranker calls the Model Plane", () => {
  const phrase = /semantic reranker[^\n]{0,160}(model plane|model-plane|model-gateway|models:invoke)/i;
  for (const { path, text } of currentDocs()) {
    assert.doesNotMatch(text, phrase, `${path} describes the reranker as a Model Plane caller`);
  }
});

// ----------------------------------------------------------------------------- rerank wiring

test("the rerank deadline is read from its env, clamped in the config and passed to the reranker", () => {
  const config = read(paths.config);
  const main = read(paths.main);
  assert.match(config, /pub rerank_deadline_ms:\s*Option<u64>/, "config.rs has the deadline field");
  assert.match(config, /QUARRY_EDGE__RERANK_DEADLINE_MS/, "the field names its env variable");
  assert.match(config, /pub fn rerank_deadline\(&self\)/, "config.rs derives the budget from the field");
  assert.match(main, /\.with_deadline\(cfg\.rerank_deadline\(\)\)/, "main.rs gives the wrapper the budget");
});

test("the rerank switches the compose sets are the ones the binary reads", () => {
  const env = environmentOf(quarryEdgeBlock(read(paths.compose)));
  // A rerank key the binary never reads is a dead switch: each must be a field of EdgeConfig in config.rs.
  const fields = new Set([...read(paths.config).matchAll(/^\s*pub ([a-z_][a-z0-9_]*)\s*:/gm)].map((m) => m[1]));
  for (const key of env.keys()) {
    if (!key.includes("RERANK")) continue;
    const field = key.replace(/^QUARRY_EDGE__/, "").toLowerCase();
    assert.ok(fields.has(field), `${key} is read by the binary (config.rs has the field ${field})`);
  }
  assert.equal(composeDefault(env.get("QUARRY_EDGE__SEMANTIC_RERANK")), "0", "rerank is dark by default until the ship check passes");
  assert.ok(env.has("QUARRY_EDGE__SEMANTIC_RERANK_TOP_N"), "TOP_N is set");
  assert.ok(env.has("QUARRY_EDGE__RERANK_SHADOW"), "shadow mode is set");
  assert.ok(["0", "1"].includes(composeDefault(env.get("QUARRY_EDGE__RERANK_SHADOW"))), "shadow defaults to 0 or 1");
  assert.equal(composeDefault(env.get("QUARRY_EDGE__RERANK_DEADLINE_MS")), "2000", "deadline default is 2000 ms");
  assert.ok(!env.has("QUARRY_EDGE__SEMANTIC_RERANK_MODEL"), "the model-plane rerank model is removed");
  assert.ok(!env.has("QUARRY_EDGE__RERANK_WINDOW"), "highlight windows are deferred, not configured");
  for (const key of env.keys()) {
    assert.ok(!/HIGHLIGHT/.test(key), `no highlight switch (${key}) is configured`);
  }
});

test("the compose rerank comments name the live check, the ship gate and the score opt-in", () => {
  const comments = quarryEdgeBlock(read(paths.compose))
    .filter((line) => /^ {6}#/.test(line))
    .join("\n");
  assert.match(comments, /qb-preflight\.sh" --live --ship-qb-t09/, "the ship check is named");
  assert.match(comments, /model-gateway[\s\S]{0,200}score_kinds|score_kinds[\s\S]{0,200}model-gateway/, "the model-gateway score opt-in (QB-T12) is named");
});

test("the Model Plane switches default off, as the docs say (QB-T17)", () => {
  const env = environmentOf(quarryEdgeBlock(read(paths.compose)));
  for (const key of [
    "QUARRY_EDGE__AUTOPROMPT",
    "QUARRY_EDGE__LLM_CLASSIFY_INTENT",
    "QUARRY_EDGE__PAGE_TITLE_ENRICH",
    "QUARRY_EDGE__SEARCH_SUGGEST",
    "QUARRY_EDGE__MODEL_AUDIO",
  ]) {
    assert.equal(composeDefault(env.get(key)), "0", `${key} defaults to 0`);
  }
});

test("the runbook and the self-host table state the rerank defaults the compose has", () => {
  const env = environmentOf(quarryEdgeBlock(read(paths.compose)));
  const rerank = composeDefault(env.get("QUARRY_EDGE__SEMANTIC_RERANK"));
  const shadow = composeDefault(env.get("QUARRY_EDGE__RERANK_SHADOW"));
  const runbook = read(paths.runbooks);
  assert.match(runbook, /QUARRY_EDGE__RERANK_DEADLINE_MS/, "RUNBOOKS names the deadline switch");
  assert.match(runbook, /QUARRY_EDGE__RERANK_SHADOW[\s\S]{0,400}smoke run/, "RUNBOOKS names the shadow smoke run");
  assert.ok(runbook.includes("`QUARRY_EDGE_SEMANTIC_RERANK` defaults to `" + rerank + "`"), "RUNBOOKS states the compose rerank default");
  assert.ok(runbook.includes("sets it to `" + shadow + "`"), "RUNBOOKS states the compose shadow default");
  const selfHost = read(paths.selfHost);
  assert.ok(selfHost.includes("(the Ingestion compose sets `" + rerank + "`"), "SELF_HOST states the compose rerank default");
  assert.ok(selfHost.includes("(the Ingestion compose sets `" + shadow + "`"), "SELF_HOST states the compose shadow default");
});

// ----------------------------------------------------------------------------- cross-plane docs

test("the CROSS_PLANE_AUTH data-plane row names every scope the granted data-plane requests carry", () => {
  const row = rowLine(read(paths.crossPlaneAuth), "quarry-edge → data-plane");
  const scopes = new Set(
    tokenRows.filter((r) => r.audience === "data-plane" && r.granted === true).flatMap((r) => r.scopes),
  );
  assert.ok(scopes.has("data:rerank"), "the manifest has the rerank row");
  // The table has four cells between the outer bars; the Auth cell (index 2) is where the scopes are named.
  const cells = row.split("|");
  assert.equal(cells.length, 6, "the data-plane row has the four table cells");
  for (const scope of scopes) {
    assert.ok(cells[2].includes(scope), `the Auth cell of the data-plane row names ${scope}`);
  }
});

test("the CROSS_PLANE_AUTH model-plane row says the mint is refused, not that it is implemented", () => {
  const row = rowLine(read(paths.crossPlaneAuth), "quarry-edge → model-plane");
  assert.match(row, /not granted/i, "the row says the model-gateway grant is absent");
  assert.doesNotMatch(row, /\|\s*implemented\s*\|/i, "the row does not claim the path is implemented");
  const modelRows = tokenRows.filter((r) => r.audience === "model-gateway");
  assert.ok(modelRows.length > 0 && modelRows.every((r) => r.granted === false), "the manifest refuses every model row");
  const audiences = registry["quarry-edge"].audiences ?? [];
  assert.ok(!audiences.includes("model-gateway"), "the committed registry gives quarry-edge no model-gateway audience");
});

test("every granted data-plane scope is in the committed registry for quarry-edge", () => {
  const row = registry["quarry-edge"];
  const granted = row.scopesByAudience?.["data-plane"] ?? row.scopes ?? [];
  for (const requested of tokenRows.filter((r) => r.audience === "data-plane" && r.granted === true)) {
    for (const scope of requested.scopes) {
      assert.ok(granted.includes(scope), `the registry grants quarry-edge ${scope} on data-plane (${requested.name})`);
    }
  }
});

test("each token-request row is named in the cross-plane integration table", () => {
  const text = read(paths.crossPlaneIntegration);
  for (const row of tokenRows) {
    assert.ok(text.includes(`\`${row.name}\``), `the integration table names ${row.name}`);
  }
});

test("the integration table does not list the semantic reranker among the Model Plane callers", () => {
  const line = read(paths.crossPlaneIntegration)
    .split(/\r?\n/)
    .find((candidate) => candidate.includes("refuses the three Model Plane rows"));
  assert.ok(line, "the Model Plane refusal sentence is present");
  assert.doesNotMatch(line, /semantic reranker/i, "the reranker no longer calls the Model Plane");
});

// ----------------------------------------------------------------------------- scrape AI formats (INNH-1)

const aiFormatsSource = () => read(join(quarry, "crates", "quarry-edge", "src", "scrape_contract", "ai_formats.rs"));

// The names the edge reads for the AI formats: the string constants of ai_formats.rs.
const aiFormatsEnvNames = () => {
  const names = [...aiFormatsSource().matchAll(/pub const ENV_[A-Z_]+: &str = "([A-Z0-9_]+)";/g)].map((m) => m[1]);
  assert.ok(names.includes("QUARRY_EDGE__AI_FORMATS"), "the code reads QUARRY_EDGE__AI_FORMATS");
  return names;
};

test("the AI formats of /v1/scrape are off in the Ingestion compose and in the deploy compose", () => {
  const deployCompose = join(quarry, "deploy", "compose", "docker-compose.yml");
  for (const [label, text] of [
    ["Ingestion compose", read(paths.compose)],
    ["deploy compose", read(deployCompose)],
  ]) {
    const env = environmentOf(quarryEdgeBlock(text));
    assert.equal(composeDefault(env.get("QUARRY_EDGE__AI_FORMATS")), "0", `${label}: QUARRY_EDGE__AI_FORMATS defaults to 0`);
    assert.equal(composeDefault(env.get("QUARRY_EDGE__AI_FORMATS_ZDR")), "refuse", `${label}: the ZDR value is refuse`);
  }
});

test("every QUARRY_EDGE__AI_FORMATS name in the docs and the composes is read by the code", () => {
  const read_by_code = new Set(aiFormatsEnvNames());
  const deploy = join(quarry, "deploy", "compose");
  const files = [
    paths.compose,
    join(deploy, "docker-compose.yml"),
    join(deploy, "README.md"),
    ...currentDocs().map((doc) => doc.path),
    paths.openapi,
  ];
  let seen = 0;
  for (const path of new Set(files)) {
    const text = read(path);
    // The compose variable form (`QUARRY_EDGE_AI_FORMATS`, one underscore) feeds the double-underscore name.
    for (const match of text.matchAll(/QUARRY_EDGE__?AI_FORMATS[A-Z0-9_]*/g)) {
      const name = match[0].replace(/_+$/, "").replace(/^QUARRY_EDGE_AI/, "QUARRY_EDGE__AI");
      seen += 1;
      assert.ok(read_by_code.has(name), `${path} names ${name}, which ai_formats.rs does not read`);
    }
  }
  assert.ok(seen >= 6, "the check found the names it is about");
});

test("the runbook and the self-host guide document every name the AI formats read, and the 501 reasons", () => {
  const runbook = read(paths.runbooks);
  const selfHost = read(paths.selfHost);
  const names = aiFormatsEnvNames();
  for (const name of ["QUARRY_EDGE__AI_FORMATS", "QUARRY_EDGE__AI_FORMATS_ZDR", "QUARRY_EDGE__MODEL_PLANE_INFERENCE_TOKEN"]) {
    assert.ok(names.includes(name), `the code reads ${name}`);
    assert.ok(selfHost.includes(name), `SELF_HOST documents ${name}`);
  }
  assert.ok(runbook.includes("QUARRY_EDGE__AI_FORMATS") || runbook.includes("QUARRY_EDGE_AI_FORMATS"), "RUNBOOKS names the switch");
  for (const reason of ["model_plane_unavailable", "model_lane_not_granted", "extract_ai_scope_required", "unsupported_in_stream"]) {
    assert.ok(runbook.includes(reason), `RUNBOOKS names ${reason}`);
    assert.ok(read(join(quarry, "docs", "ERROR_REASONS.md")).includes(reason), `ERROR_REASONS names ${reason}`);
  }
});

// ----------------------------------------------------------------------------- no phantom variables (INNH-3)

// A name the docs and the 501 hints tell an operator to set must be one the code reads. The AI formats
// switch was named for /v1/extract in an audit and in the brief before QB-T17 made it the gate; this guard is
// the one that would have caught a name like that.

const rustFiles = (dir, out = []) => {
  for (const name of readdirSync(dir, { withFileTypes: true })) {
    const path = join(dir, name.name);
    if (name.isDirectory()) {
      if (name.name !== "target" && name.name !== "tests") rustFiles(path, out);
    } else if (name.name.endsWith(".rs")) {
      out.push(path);
    }
  }
  return out;
};

// The names the code reads: every EdgeConfig field (the config crate maps QUARRY_EDGE__<FIELD> onto it) and
// every QUARRY_EDGE__ string literal in the crates' sources (the switches read with std::env).
const namesTheCodeReads = () => {
  const names = new Set();
  const config = read(paths.config).replace(/\r/g, "");
  const struct = /pub struct EdgeConfig \{([\s\S]*?)\n\}/.exec(config);
  assert.ok(struct, "config.rs defines EdgeConfig");
  for (const field of struct[1].matchAll(/^\s*pub ([a-z0-9_]+):/gm)) names.add(`QUARRY_EDGE__${field[1].toUpperCase()}`);
  for (const crate of readdirSync(join(quarry, "crates"))) {
    const src = join(quarry, "crates", crate, "src");
    if (!existsSync(src)) continue;
    for (const file of rustFiles(src)) {
      for (const literal of readFileSync(file, "utf8").matchAll(/"(QUARRY_EDGE__[A-Z0-9_]+)"/g)) names.add(literal[1]);
    }
  }
  return names;
};

// The names a compose file interpolates (`${MODEL_PLANE_URL:-}`): the spelling an operator types for the stack.
const composeVariables = (text) => new Set([...text.matchAll(/\$\{([A-Z][A-Z0-9_]*)/g)].map((m) => m[1]));

// Names in `text` that the code does not read. `QUARRY_EDGE_X` (one underscore, the compose spelling) is folded to
// `QUARRY_EDGE__X`. A bare MODEL_PLANE_URL / MODEL_PLANE_TOKEN / MODEL_PLANE_INFERENCE_TOKEN must be a compose variable.
function phantomNames(text, readByCode, compose) {
  const phantoms = new Set();
  for (const match of text.matchAll(/\bQUARRY_EDGE_{1,2}[A-Z][A-Z0-9_]*/g)) {
    const name = match[0].replace(/_+$/, "").replace(/^QUARRY_EDGE_{1,2}/, "QUARRY_EDGE__");
    if (!readByCode.has(name)) phantoms.add(match[0]);
  }
  for (const match of text.matchAll(/(?<![A-Za-z0-9_])MODEL_PLANE_(?:URL|TOKEN|INFERENCE_TOKEN)(?![A-Za-z0-9_])/g)) {
    if (!compose.has(match[0])) phantoms.add(match[0]);
  }
  return [...phantoms];
}

// The lines of a section: from the heading that matches to the next heading of the same or a higher level.
function sectionOf(text, heading, level) {
  const lines = text.split(/\r?\n/);
  const start = lines.findIndex((line) => line.startsWith(`${level} `) && heading.test(line));
  assert.notEqual(start, -1, `a "${level}" section matching ${heading} exists`);
  let end = lines.length;
  for (let i = start + 1; i < lines.length; i++) {
    if (new RegExp(`^#{1,${level.length}} `).test(lines[i])) {
      end = i;
      break;
    }
  }
  return lines.slice(start, end).join("\n");
}

const hintsOfExtract = () => {
  const source = read(join(quarry, "crates", "quarry-edge", "src", "extract_routes.rs"));
  const cut = source.indexOf("#[cfg(test)]");
  assert.ok(cut > 0, "extract_routes.rs has a test module");
  return source.slice(0, cut);
};

test("the phantom-name check tells a real setting from one the code does not read", () => {
  const known = new Set(["QUARRY_EDGE__MODEL_PLANE_URL", "QUARRY_EDGE__AI_FORMATS"]);
  const compose = new Set(["MODEL_PLANE_URL"]);
  assert.deepEqual(phantomNames("set QUARRY_EDGE__MODEL_PLANE_URL and QUARRY_EDGE_AI_FORMATS, or MODEL_PLANE_URL", known, compose), []);
  assert.deepEqual(phantomNames("set QUARRY_EDGE__EXTRACT_AI_FORMATS=1.", known, compose), ["QUARRY_EDGE__EXTRACT_AI_FORMATS"]);
  assert.deepEqual(phantomNames("export MODEL_PLANE_TOKEN", known, compose), ["MODEL_PLANE_TOKEN"]);
  assert.deepEqual(phantomNames("QUARRY_EDGE__AI_FORMATS_NOPE_ and QUARRY_EDGE_NOPE", known, compose).sort(), ["QUARRY_EDGE_NOPE", "QUARRY_EDGE__AI_FORMATS_NOPE_"]);
});

test("every variable the Model Plane docs, the openapi, the composes and the extract hints name is read by the code (INNH-3)", () => {
  const readByCode = namesTheCodeReads();
  assert.ok(readByCode.has("QUARRY_EDGE__MODEL_PLANE_URL") && readByCode.has("QUARRY_EDGE__MODEL_PLANE_TOKEN"), "the code reads the two Model Plane settings");
  assert.ok(readByCode.has("QUARRY_EDGE__AI_FORMATS"), "the code reads the AI formats switch");
  const deployCompose = join(quarry, "deploy", "compose", "docker-compose.yml");
  const compose = new Set([...composeVariables(read(paths.compose)), ...composeVariables(read(deployCompose))]);
  const runbooks = read(paths.runbooks);
  // Whole files that carry no unrelated switch, and the sections of the long ones that are about the Model Plane. The
  // composes are checked on the lines that name the Model Plane or the AI formats.
  const composeLines = (path) =>
    read(path)
      .split(/\r?\n/)
      .filter((line) => /MODEL_PLANE|AI_FORMATS/.test(line))
      .join("\n");
  const checked = [
    ["extract_routes.rs (the 501 texts)", hintsOfExtract()],
    ["docs/openapi.yaml", read(paths.openapi)],
    ["docs/CONTRACTS.md", read(join(paths.docs, "CONTRACTS.md"))],
    ["docs/ERROR_REASONS.md", read(join(paths.docs, "ERROR_REASONS.md"))],
    ["deploy/compose/README.md", read(join(quarry, "deploy", "compose", "README.md"))],
    ["RUNBOOKS: Enable structured extract and AI formats", sectionOf(runbooks, /Enable structured extract and AI formats/, "##")],
    ["RUNBOOKS: Scrape AI formats", sectionOf(runbooks, /Scrape AI formats/, "##")],
    ["RUNBOOKS: /v1/extract with a prompt", sectionOf(runbooks, /`\/v1\/extract` with a `prompt`/, "##")],
    ["SELF_HOST: Quarry to Model Plane", sectionOf(read(paths.selfHost), /Quarry → Model Plane/, "###")],
    ["Ingestion compose (Model Plane lines)", composeLines(paths.compose)],
    ["deploy compose (Model Plane lines)", composeLines(deployCompose)],
  ];
  for (const [label, text] of checked) {
    assert.deepEqual(phantomNames(text, readByCode, compose), [], `${label} names a variable the code does not read`);
  }
});

test("the extract 501 texts name the real Model Plane URL setting and the AI formats switch only in its own text", () => {
  const hints = hintsOfExtract();
  assert.ok(hints.includes("QUARRY_EDGE__MODEL_PLANE_URL"), "the 501 names QUARRY_EDGE__MODEL_PLANE_URL");
  assert.ok(hints.includes("Enable structured extract and AI formats"), "the 501 points at the RUNBOOKS section");
  // QB-T17 (main) gates /v1/extract with QUARRY_EDGE__AI_FORMATS, so the switch off has its own 501 text; the two
  // texts for a missing URL and a missing credential name no switch.
  assert.ok(hints.includes("AI_FORMATS_OFF_ERROR"), "extract has a 501 text for the AI formats switch being off");
  assert.ok(sectionOf(read(paths.runbooks), /Enable structured extract and AI formats/, "##").length > 500, "the RUNBOOKS section exists");
});

test("the RUNBOOKS section says there is no production path before WS16.S1 to S3 and decision X-7", () => {
  const section = sectionOf(read(paths.runbooks), /Enable structured extract and AI formats/, "##");
  assert.match(section, /no production path today/i);
  assert.ok(section.includes("WS16.S1 to S3") && section.includes("X-7") && section.includes("OD-1"), "names WS16.S1 to S3, X-7 and OD-1");
  assert.match(section, /gates `\/v1\/extract`/, "says the AI formats switch gates extract (QB-T17)");
  for (const state of ["No Model Plane URL", "The AI formats switch is off", "A URL, but nothing to authenticate with", "A URL and a credential, but the gateway refuses"]) {
    assert.ok(section.includes(state), `the section describes the state: ${state}`);
  }
});

// ----------------------------------------------------------------------------- answer synthesis (INNH-8a)

const ANSWER_REASONS = [
  "synthesis_disabled",
  "zdr_unavailable",
  "model_path_refused",
  "no_search_provider",
  "no_model_plane_url",
  "model_client_init_failed",
  "model_plane_auth_refused",
  "model_plane_rate_limited",
  "model_plane_error",
  "answer_search_failed",
];

test("answer synthesis is off in the Ingestion compose and in the deploy compose, and the code reads the switch", () => {
  const deployCompose = join(quarry, "deploy", "compose", "docker-compose.yml");
  for (const [label, text] of [
    ["Ingestion compose", read(paths.compose)],
    ["deploy compose", read(deployCompose)],
  ]) {
    const env = environmentOf(quarryEdgeBlock(text));
    assert.equal(composeDefault(env.get("QUARRY_EDGE__ANSWER_SYNTHESIS")), "0", `${label}: QUARRY_EDGE__ANSWER_SYNTHESIS defaults to 0`);
  }
  const status = read(join(quarry, "crates", "quarry-edge", "src", "answer_status.rs"));
  // One switch, one gate: the QB-T17 boot gate owns QUARRY_EDGE__ANSWER_SYNTHESIS, and the answer
  // status is derived from its decision. A second env constant or config field would be a second gate.
  assert.doesNotMatch(status, /ENV_ANSWER_SYNTHESIS|"QUARRY_EDGE__ANSWER_SYNTHESIS"/, "answer_status.rs defines no switch of its own");
  assert.match(status, /from_decision\(/, "the answer status is derived from the boot gate's decision");
  const gate = read(join(quarry, "crates", "quarry-edge", "src", "model_plane_gate.rs"));
  assert.match(gate, /Self::AnswerSynthesis => "QUARRY_EDGE__ANSWER_SYNTHESIS"/, "the boot gate names the switch");
  const config = read(join(quarry, "crates", "quarry-edge", "src", "config.rs"));
  assert.match(config, /pub answer_synthesis: bool,/, "EdgeConfig reads it");
  assert.equal((config.match(/pub answer_synthesis: bool,/g) || []).length, 1, "EdgeConfig has the field once");
});

test("every QUARRY_EDGE__ANSWER_SYNTHESIS name in the docs and the composes is the one the code reads", () => {
  const deploy = join(quarry, "deploy", "compose");
  const files = [paths.compose, join(deploy, "docker-compose.yml"), join(deploy, "README.md"), ...currentDocs().map((doc) => doc.path), paths.openapi];
  let seen = 0;
  for (const path of new Set(files)) {
    for (const match of read(path).matchAll(/QUARRY_EDGE__?ANSWER_SYNTHESIS[A-Z0-9_]*/g)) {
      const name = match[0].replace(/_+$/, "").replace(/^QUARRY_EDGE_ANSWER/, "QUARRY_EDGE__ANSWER");
      seen += 1;
      assert.equal(name, "QUARRY_EDGE__ANSWER_SYNTHESIS", `${path} names ${name}, which the code does not read`);
    }
  }
  assert.ok(seen >= 6, "the check found the names it is about");
});

test("the runbook, the contract and the error reasons name every answer reason the code can send, and only those", () => {
  const status = read(join(quarry, "crates", "quarry-edge", "src", "answer_status.rs"));
  const inCode = [...status.matchAll(/Self::[A-Za-z]+ => "([a-z_]+)",/g)].map((m) => m[1]);
  assert.deepEqual(new Set(inCode), new Set(ANSWER_REASONS), "the reasons in answer_status.rs are the documented set");
  const runbook = read(paths.runbooks);
  const contracts = read(join(quarry, "docs", "CONTRACTS.md"));
  const reasons = read(join(quarry, "docs", "ERROR_REASONS.md"));
  const spec = read(paths.openapi);
  for (const reason of ANSWER_REASONS) {
    assert.ok(reasons.includes("`" + reason + "`"), `ERROR_REASONS names ${reason}`);
    assert.ok(contracts.includes(reason), `CONTRACTS names ${reason}`);
    assert.ok(spec.includes("- " + reason), `openapi.yaml lists ${reason}`);
  }
  for (const reason of ["synthesis_disabled", "model_path_refused", "zdr_unavailable", "no_search_provider", "no_model_plane_url", "model_client_init_failed", "model_plane_auth_refused"]) {
    assert.ok(runbook.includes(reason), `RUNBOOKS names ${reason}`);
  }
  assert.ok(runbook.includes("## Answer synthesis: enable, disable and the reasons"), "RUNBOOKS has the section the 501 hints point at");
  assert.ok(runbook.includes("QUARRY_EDGE_ANSWER_SYNTHESIS=1"), "RUNBOOKS gives the switch to set");
  assert.ok(read(paths.selfHost).includes("QUARRY_EDGE__ANSWER_SYNTHESIS"), "SELF_HOST documents the switch");
});

// ----------------------------------------------------------------------------- openapi

test("SearchRequest documents zdr and score_kinds", () => {
  const spec = read(paths.openapi);
  const properties = schemaProperties(spec, "SearchRequest");
  assert.ok(properties.has("zdr"), "zdr is documented");
  assert.ok(properties.has("score_kinds"), "score_kinds is documented");
  assert.ok(
    properties.get("score_kinds").join("\n").includes("cross_encoder.v1"),
    "score_kinds names cross_encoder.v1",
  );
});

test("SearchResponse documents score_kind and the rerank report", () => {
  const spec = read(paths.openapi);
  const properties = schemaProperties(spec, "SearchResponse");
  assert.ok(properties.has("score_kind"), "score_kind is documented");
  assert.ok(properties.has("rerank"), "rerank is documented");
  assert.ok(
    properties.get("rerank").join("\n").includes("#/components/schemas/RerankReport"),
    "rerank refers to the RerankReport schema",
  );
});

test("RerankReport lists the five statuses the wire carries", () => {
  const spec = read(paths.openapi);
  const properties = schemaProperties(spec, "RerankReport");
  assert.ok(properties.has("status") && properties.has("reason") && properties.has("profile"));
  const status = properties.get("status").join("\n");
  for (const value of ["applied", "skipped", "degraded", "shadow", "rerank-not-configured"]) {
    assert.ok(status.includes(value), `status names ${value}`);
  }
});

test("SearchResultItem documents the per-result score and no highlights", () => {
  const spec = read(paths.openapi);
  const properties = schemaProperties(spec, "SearchResultItem");
  assert.ok(properties.has("score"), "score is documented");
  assert.ok(!properties.has("highlights"), "rerank highlights are deferred (QB-T06), not documented");
});

test("the /v1/search operation documents its 400 refusals", () => {
  const block = pathBlock(read(paths.openapi), "/v1/search");
  assert.ok(block.includes("'400':"), "the 400 response (ZDR_REQUIRED, SCORE_KIND_UNSUPPORTED) is documented");
});

// ----------------------------------------------------------------------------- whole-file parse

test("openapi.yaml parses as one YAML document with no duplicate keys", () => {
  const text = read(paths.openapi);
  try {
    parseYamlSubset(text);
  } catch (error) {
    assert.fail(`openapi.yaml does not parse as one YAML document: ${error.message}`);
  }
});

test("the whole-file check refuses a duplicate key and accepts a folded quoted scalar", () => {
  assert.throws(() => parseYamlSubset("a:\n  b: 1\n  b: 2\n"), /duplicate key "b"/);
  assert.doesNotThrow(() => parseYamlSubset("a:\n  d: 'one\n\n    two'\n  e:\n  - x\n"));
});

// ----------------------------------------------------------------------------- change text diff (INNH-4)

test("the change text diff switch is read by the code, documented, and not turned on in a compose", () => {
  const source = read(join(quarry, "crates", "quarry-edge", "src", "change_diff.rs"));
  assert.match(source, /pub const ENV_CHANGE_TEXT_DIFF: &str = "QUARRY_EDGE__CHANGE_TEXT_DIFF";/, "the code reads the switch");
  const name = "QUARRY_EDGE__CHANGE_TEXT_DIFF";
  const docs = [
    ["RUNBOOKS", paths.runbooks],
    ["CONTRACTS", join(quarry, "docs", "CONTRACTS.md")],
    ["openapi", paths.openapi],
    ["openapi-internal", join(quarry, "docs", "openapi-internal.yaml")],
  ];
  for (const [label, path] of docs) {
    const text = read(path);
    assert.ok(text.includes(name), `${label} names ${name}`);
    // Every spelling of the switch in a document is the one the code reads.
    for (const match of text.matchAll(/QUARRY_EDGE__?CHANGE_TEXT_[A-Z0-9_]*/g)) {
      assert.equal(match[0].replace(/_+$/, ""), name, `${label} names ${match[0]}, which the code does not read`);
    }
  }
  // Dark by default: a compose either leaves it out or sets it to 0.
  for (const compose of [paths.compose, join(quarry, "deploy", "compose", "docker-compose.yml")]) {
    const value = environmentOf(quarryEdgeBlock(read(compose))).get(name);
    assert.ok(value === undefined || composeDefault(value) === "0", `${compose} does not turn the text diff on`);
  }
});

// ----------------------------------------------------------------------------- webhook subscriptions (INNH-5)

test("the webhook subscription switches are read by the code, documented, and not turned on in a compose", () => {
  const edgeSource = read(join(quarry, "crates", "quarry-edge", "src", "webhook_subscription_routes.rs"));
  const controlSource = read(join(quarry, "services", "quarry-control", "internal", "resources", "webhook_subscriptions.go"));
  const edgeName = "QUARRY_EDGE__WEBHOOK_SUBSCRIPTIONS";
  const controlName = "QUARRY_CONTROL__WEBHOOK_SUBSCRIPTIONS";
  assert.ok(edgeSource.includes(`pub const ENV_WEBHOOK_SUBSCRIPTIONS: &str = "${edgeName}";`), "the edge reads its switch");
  assert.ok(controlSource.includes(`getenv("${controlName}")`), "control reads its switch");
  const docs = [
    ["RUNBOOKS", paths.runbooks],
    ["CONTRACTS", join(quarry, "docs", "CONTRACTS.md")],
    ["openapi", paths.openapi],
  ];
  for (const [label, path] of docs) {
    const text = read(path);
    for (const name of [edgeName, controlName]) {
      assert.ok(text.includes(name), `${label} names ${name}`);
    }
    // Every spelling of a switch in a document is one the code reads.
    for (const match of text.matchAll(/QUARRY_(?:EDGE|CONTROL)__?WEBHOOK_SUBSCRIPTION[A-Z0-9_]*/g)) {
      const spelled = match[0].replace(/_+$/, "");
      assert.ok([edgeName, controlName].includes(spelled), `${label} names ${match[0]}, which the code does not read`);
    }
  }
  // Dark by default: a compose leaves both out (edge: unset or 0).
  for (const compose of [paths.compose, join(quarry, "deploy", "compose", "docker-compose.yml")]) {
    const text = read(compose);
    assert.ok(!text.includes(controlName), `${compose} does not set the control switch`);
    const value = environmentOf(quarryEdgeBlock(text)).get(edgeName);
    assert.ok(value === undefined || composeDefault(value) === "0", `${compose} does not turn the subscriptions on`);
  }
});
