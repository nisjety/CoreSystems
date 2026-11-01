// Route extraction and OpenAPI path parsing for the route-coverage test.
//
// Quarry registers its HTTP surface with `.route("<path>", ...)` calls in a few
// Rust files. This module reads those string literals (not a parse of the Rust:
// a route built by a helper, or from a constant, is invisible to it, which is
// why the test also has a "routes found" floor and a pinned count) and the
// `paths` keys of docs/openapi.yaml, with `:param` and `{param}` normalized so
// the two spellings compare equal.

import fs from 'node:fs';
import path from 'node:path';

/** The source files that register routes, relative to the Quarry-v2 root. */
export function routeSourceFiles(root) {
  const src = path.join(root, 'crates/quarry-edge/src');
  const files = ['routes.rs', 'fleet_routes.rs', 'queue_routes.rs'].map((name) => path.join(src, name));
  // agent_routes.rs was split into agent_routes/*.rs (WS2.B); follow whichever exists.
  const split = path.join(src, 'agent_routes');
  if (fs.existsSync(split)) {
    for (const name of fs.readdirSync(split).sort()) {
      if (name.endsWith('.rs')) files.push(path.join(split, name));
    }
  } else if (fs.existsSync(path.join(src, 'agent_routes.rs'))) {
    files.push(path.join(src, 'agent_routes.rs'));
  }
  return files.filter((file) => fs.existsSync(file));
}

/** Everything before a top-level `#[cfg(test)] mod`: test-only routes are not the surface. */
export function productionPart(source) {
  const text = source.replace(/\r\n/g, '\n');
  const cut = text.search(/\n[ \t]*#\[cfg\(test\)\]\s*\n[ \t]*mod /);
  return cut >= 0 ? text.slice(0, cut) : text;
}

/** The `.route("...")` literals in one Rust source, in file order. */
export function routesIn(source) {
  const found = [];
  for (const match of productionPart(source).matchAll(/\.route\(\s*"([^"]+)"/g)) {
    found.push(match[1]);
  }
  return found;
}

/** `:id` and `{id}` both become `{}`, so `/v1/x/:run_id` equals `/v1/x/{id}`. */
export function normalize(routePath) {
  return routePath.replace(/:[A-Za-z_][A-Za-z0-9_]*/g, '{}').replace(/\{[^}]*\}/g, '{}');
}

/** Every route the edge registers, as a sorted list of distinct normalized paths. */
export function registeredPaths(root) {
  const paths = new Set();
  let registrations = 0;
  for (const file of routeSourceFiles(root)) {
    for (const route of routesIn(fs.readFileSync(file, 'utf8'))) {
      registrations += 1;
      paths.add(normalize(route));
    }
  }
  return { paths: [...paths].sort(), registrations };
}

/**
 * The /v1/internal routes of the Go control service (D2-050): `r.Get("/v1/internal/...", ...)` and the like in
 * services/quarry-control/internal/resources. Returns sorted "METHOD normalized-path" entries.
 */
export function controlInternalRoutes(root) {
  const dir = path.join(root, 'services/quarry-control/internal/resources');
  const found = new Set();
  for (const name of fs.readdirSync(dir).sort()) {
    if (!name.endsWith('.go') || name.endsWith('_test.go')) continue;
    const source = fs.readFileSync(path.join(dir, name), 'utf8').replace(/\r\n/g, '\n');
    for (const match of source.matchAll(/\br\.(Get|Post|Put|Patch|Delete)\(\s*"(\/v1\/internal\/[^"]+)"/g)) {
      found.add(`${match[1].toUpperCase()} ${normalize(match[2])}`);
    }
  }
  return [...found].sort();
}

/** The keys of `paths:` in openapi.yaml, normalized. */
export function documentedPaths(openapiText) {
  const text = openapiText.replace(/\r\n/g, '\n');
  const start = text.indexOf('\npaths:\n');
  if (start < 0) throw new Error('openapi.yaml has no paths: section');
  const rest = text.slice(start + '\npaths:\n'.length);
  const end = rest.search(/\n[A-Za-z]/);
  const section = end >= 0 ? rest.slice(0, end) : rest;
  const paths = new Set();
  for (const match of section.matchAll(/^  (\/[^\s:]*(?:\{[^}]*\}[^\s:]*)*):\s*$/gm)) {
    paths.add(normalize(match[1]));
  }
  return [...paths].sort();
}
