# Bulk job price estimate (E3.7.8)

Status: built, dark. Nothing calls the function yet. The job gate (E3.7.9)
and the job record (WS15 L1) are the first intended callers, and neither is
wired by this slice.

## What it is

`crates/quarry-runtime/src/job_pricing.rs` holds `estimate_job_price`, a pure
function. It takes a `JobPriceInput` (kind, fetch units, browser units,
attempts per unit) and a `JobRates`, and returns a `JobPrice` in micro-USD.
It does no I/O, reads no environment, and never sees a URL or page content.
Every result is labelled `CostSource::Estimated`.

## Units and rules

- `pages` is the number of HTTP fetch units. For batch and crawl these are
  pages. For map they are sitemap documents, because map reads sitemaps and
  never renders a page. For search hydration they are result pages.
- `browser_pages` is the number of fetch units expected to render in a browser.
  It is forced to zero for `map`.
- Each unit pays one static fetch per attempt. Each browser unit also pays
  browser time per attempt.
- Zero attempts means one. Attempts above 5 (Temporal's five attempts) are
  clamped to 5.
- Pages above 5000 (the crawl page ceiling, X-12) are clamped to 5000. Browser
  units above `pages` are clamped to `pages`.
- Any clamp sets `clamped: true`. Arithmetic saturates and never wraps.

## Rates are placeholders

`JobRates::default()` is not a price list.

| Rate | Default | Basis |
|---|---|---|
| `static_fetch_micro_usd` | 1000 (0.001 USD) | the flat table's `navigate` cost in `action_cost.rs` |
| `browser_second_micro_usd` | 30 (0.00003 USD) | `BUDGET_BROWSER_SECOND_USD` in quarry-edge `budget.rs` |
| `browser_seconds_per_page` | 10 | placeholder, not measured |

The ratified numbers (X-12) and billing-core's price table (L4) replace these.
A unit test pins the static-fetch default to the flat table, so a change to
the table fails it.

## Tests

Unit tests live in `job_pricing.rs` (`mod tests`, 12 cases). They cover the
defaults, static and browser terms, retries, zero and ceiling attempts, both
page ceilings, the map rule, the empty job and saturating overflow. They were
compile-checked only. The test suite has not been run.

## Not done

- No caller. Wiring into the job gate (E3.7.9, needs the approval decision) and
  the job record (WS15 L1) waits for those slices.
- No `from_env` for `JobRates`. Rates are constants until X-12 is ratified.
- No page-count approval threshold. That belongs to the crawl-approval
  decision, not this function.
