// Package demo is the logic behind the demo endpoints: take a company profile, read the notices the
// raw store holds, parse and normalise each, and assess it against the profile with the gate and
// the fit of internal/match. The HTTP layer (internal/api) only decodes requests and renders
// results.
//
// Everything is computed per request from the evidence the raw store keeps. Nothing is stored: not
// the profile, which is a tenant's own business data, and not the parsed notices, which are
// knowledge and belong to Data Plane v2 (docs/ARCHITECTURE.md, decision P1). A notice whose body
// has been erased is never read, because the store reads raw_documents_live. This is a demo
// surface: its shapes are not a frozen contract.
package demo
