package store

// CreditsPerPage is the credit price of one fetched page. It is the single
// price both credit figures use: the per-job credits on the job reads
// (resources.creditsFor) and the team aggregate (TeamUsageStore.CreditUsage,
// both the in-memory and the Postgres implementation), so the two cannot drift.
//
// It is a flat Quarry-side estimate, not what billing-core invoices: a
// browser-rendered page weighs the same as a plain fetch, and a failed or
// blocked page is free. There is no Rust constant to mirror; the Rust side
// (crates/quarry-core/src/job_pricing.rs) prices estimates in micro-USD and is
// a separate, unwired module.
const CreditsPerPage = 1.0

// CreditEventType is the one event type that earns credits: the orchestrator
// emits a page_fetched event for each page it fetched.
const CreditEventType = "page_fetched"
