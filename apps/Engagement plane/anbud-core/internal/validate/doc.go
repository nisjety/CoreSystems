// Package validate will hold the deterministic validation gates that run
// before any AI sees a notice: schema and code-list conformance, cross-source
// reconciliation (the notice XML against Doffin's search record and against
// TED), and lifecycle consistency.
//
// The output is a machine-readable Report with per-field findings. The AI
// validation gate (a Model Plane work item) consumes the Report as evidence; it
// never replaces it and never rewrites canonical fields.
//
// Not implemented yet. See docs/ARCHITECTURE.md, "Build order".
package validate
