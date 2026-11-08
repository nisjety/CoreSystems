//! Price estimate for bulk acquisition jobs: batch, crawl, map and search
//! hydration (plan E3.7, slice 8, pricing function).
//!
//! Pure arithmetic: no I/O, no environment variables, no clock. The result is
//! an estimate and is labelled [`CostSource::Estimated`]. The rates in
//! [`JobRates::default`] are placeholders, not prices. They follow the flat
//! action table (`action_cost.rs`) and the browser-second rate of the agent
//! budget (`quarry-edge` `budget.rs`), pending the ratified budget numbers
//! (plan decision X-12) and billing-core's price table (plan L4).
//!
//! Units. `pages` counts HTTP fetch units: pages for batch and crawl, sitemap
//! documents for map (which reads sitemaps and never renders a page), and
//! result pages for search hydration. `browser_pages` counts the fetch units
//! expected to render in a browser. It is always zero for map.
//!
//! Rules:
//! - Each fetch unit pays one static fetch per attempt.
//! - Each browser-rendered unit also pays browser time per attempt.
//! - Zero attempts means one attempt. Attempts above [`MAX_PRICED_ATTEMPTS`]
//!   (Temporal's five attempts) are clamped to it.
//! - Pages above [`MAX_PRICED_PAGES`] (the crawl page ceiling, X-12) are clamped
//!   to it. Browser units above `pages` are clamped to `pages`.
//! - Every clamp sets [`JobPrice::clamped`], so a caller can tell that the
//!   estimate covers less than the request asked for.
//! - Arithmetic saturates and never wraps.
//!
//! Nothing here reads, stores or logs URLs or page content.

use serde::{Deserialize, Serialize};

use crate::action_cost::CostSource;

/// Crawl page ceiling (plan X-12). Requests above it are priced at it.
pub const MAX_PRICED_PAGES: u32 = 5_000;

/// Attempts per fetch unit, first try included (Temporal's five attempts).
pub const MAX_PRICED_ATTEMPTS: u32 = 5;

/// Per-unit rates in micro-USD (1 USD = 1_000_000 micro-USD).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct JobRates {
    /// One static fetch. Placeholder: the flat table's `navigate` cost, 0.001 USD.
    pub static_fetch_micro_usd: u64,
    /// One second of browser time. Placeholder: `BUDGET_BROWSER_SECOND_USD`, 0.00003 USD.
    pub browser_second_micro_usd: u64,
    /// Browser seconds a rendered unit holds a browser. Placeholder, not measured.
    pub browser_seconds_per_page: u32,
}

impl Default for JobRates {
    fn default() -> Self {
        Self {
            static_fetch_micro_usd: 1_000,
            browser_second_micro_usd: 30,
            browser_seconds_per_page: 10,
        }
    }
}

/// The kind of bulk job being priced.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BulkJobKind {
    Batch,
    Crawl,
    /// Reads sitemaps over HTTP only; never renders a browser.
    Map,
    /// Hydrates search result pages.
    SearchHydration,
}

/// What the caller asks to price.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct JobPriceInput {
    pub kind: BulkJobKind,
    /// Fetch units the job may use (see the module docs for the unit per kind).
    pub pages: u32,
    /// Of those units, the ones expected to render in a browser.
    pub browser_pages: u32,
    /// Attempts per fetch unit. Zero means one.
    pub attempts_per_page: u32,
}

/// The estimate, with the bounds that were applied.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct JobPrice {
    /// Fetch units priced, after the page ceiling.
    pub pages_priced: u32,
    /// Browser-rendered units priced, after the `pages` bound and the map rule.
    pub browser_pages_priced: u32,
    /// Attempts per unit priced, after zero and the attempt ceiling.
    pub attempts_priced: u32,
    pub fetch_micro_usd: u64,
    pub browser_micro_usd: u64,
    pub total_micro_usd: u64,
    /// True when a request value was above a ceiling and was reduced to it.
    pub clamped: bool,
    pub source: CostSource,
}

/// Estimate the cost of a bulk job from its shape and the given rates.
pub fn estimate_job_price(input: JobPriceInput, rates: JobRates) -> JobPrice {
    let mut clamped = false;

    let pages = if input.pages > MAX_PRICED_PAGES {
        clamped = true;
        MAX_PRICED_PAGES
    } else {
        input.pages
    };

    let attempts = match input.attempts_per_page {
        0 => 1,
        n if n > MAX_PRICED_ATTEMPTS => {
            clamped = true;
            MAX_PRICED_ATTEMPTS
        }
        n => n,
    };

    // Map reads sitemap documents over HTTP and never renders a page, so it
    // prices no browser time whatever the caller sends.
    let requested_browser = match input.kind {
        BulkJobKind::Map => 0,
        _ => input.browser_pages,
    };
    let browser_pages = if requested_browser > pages {
        clamped = true;
        pages
    } else {
        requested_browser
    };

    let fetch_micro_usd = u64::from(pages)
        .saturating_mul(u64::from(attempts))
        .saturating_mul(rates.static_fetch_micro_usd);
    let browser_micro_usd = u64::from(browser_pages)
        .saturating_mul(u64::from(attempts))
        .saturating_mul(u64::from(rates.browser_seconds_per_page))
        .saturating_mul(rates.browser_second_micro_usd);

    JobPrice {
        pages_priced: pages,
        browser_pages_priced: browser_pages,
        attempts_priced: attempts,
        fetch_micro_usd,
        browser_micro_usd,
        total_micro_usd: fetch_micro_usd.saturating_add(browser_micro_usd),
        clamped,
        source: CostSource::Estimated,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::action_cost::estimate_action_cost;

    fn input(kind: BulkJobKind, pages: u32, browser_pages: u32, attempts: u32) -> JobPriceInput {
        JobPriceInput {
            kind,
            pages,
            browser_pages,
            attempts_per_page: attempts,
        }
    }

    #[test]
    fn default_rates_follow_the_flat_table_and_the_browser_second_rate() {
        let navigate_micro = (estimate_action_cost("navigate", 0).total_usd * 1_000_000.0).round();
        assert_eq!(JobRates::default().static_fetch_micro_usd as f64, navigate_micro);
        // quarry-edge budget.rs BUDGET_BROWSER_SECOND_USD = 0.00003 USD.
        assert_eq!(JobRates::default().browser_second_micro_usd, 30);
    }

    #[test]
    fn static_batch_pays_one_fetch_per_page_per_attempt() {
        let price = estimate_job_price(input(BulkJobKind::Batch, 10, 0, 1), JobRates::default());
        assert_eq!(price.fetch_micro_usd, 10_000);
        assert_eq!(price.browser_micro_usd, 0);
        assert_eq!(price.total_micro_usd, 10_000);
        assert_eq!(price.pages_priced, 10);
        assert_eq!(price.browser_pages_priced, 0);
        assert_eq!(price.attempts_priced, 1);
        assert!(!price.clamped);
    }

    #[test]
    fn browser_pages_add_browser_time_on_top_of_the_fetch() {
        let rates = JobRates::default();
        let price = estimate_job_price(input(BulkJobKind::Crawl, 2, 2, 1), rates);
        // 2 fetches at 1_000, plus 2 pages x 10 s x 30 micro-USD per second.
        assert_eq!(price.fetch_micro_usd, 2_000);
        assert_eq!(price.browser_micro_usd, 600);
        assert_eq!(price.total_micro_usd, 2_600);
    }

    #[test]
    fn retries_multiply_both_terms() {
        let one = estimate_job_price(input(BulkJobKind::Crawl, 4, 2, 1), JobRates::default());
        let three = estimate_job_price(input(BulkJobKind::Crawl, 4, 2, 3), JobRates::default());
        assert_eq!(three.fetch_micro_usd, one.fetch_micro_usd * 3);
        assert_eq!(three.browser_micro_usd, one.browser_micro_usd * 3);
        assert_eq!(three.total_micro_usd, one.total_micro_usd * 3);
    }

    #[test]
    fn zero_attempts_means_one_and_is_not_a_clamp() {
        let zero = estimate_job_price(input(BulkJobKind::Batch, 5, 0, 0), JobRates::default());
        let one = estimate_job_price(input(BulkJobKind::Batch, 5, 0, 1), JobRates::default());
        assert_eq!(zero.total_micro_usd, one.total_micro_usd);
        assert_eq!(zero.attempts_priced, 1);
        assert!(!zero.clamped);
    }

    #[test]
    fn attempts_above_the_ceiling_are_clamped() {
        let price = estimate_job_price(input(BulkJobKind::Batch, 1, 0, 99), JobRates::default());
        assert_eq!(price.attempts_priced, MAX_PRICED_ATTEMPTS);
        assert_eq!(price.fetch_micro_usd, 5_000);
        assert!(price.clamped);
    }

    #[test]
    fn pages_above_the_crawl_ceiling_are_clamped() {
        let price = estimate_job_price(input(BulkJobKind::Crawl, 6_000, 0, 1), JobRates::default());
        assert_eq!(price.pages_priced, MAX_PRICED_PAGES);
        assert_eq!(price.fetch_micro_usd, u64::from(MAX_PRICED_PAGES) * 1_000);
        assert!(price.clamped);
    }

    #[test]
    fn browser_pages_are_bounded_by_pages() {
        let price = estimate_job_price(input(BulkJobKind::Crawl, 3, 10, 1), JobRates::default());
        assert_eq!(price.browser_pages_priced, 3);
        assert_eq!(price.browser_micro_usd, 3 * 300);
        assert!(price.clamped);
    }

    #[test]
    fn map_never_prices_browser_time() {
        let price = estimate_job_price(input(BulkJobKind::Map, 4, 4, 1), JobRates::default());
        assert_eq!(price.browser_pages_priced, 0);
        assert_eq!(price.browser_micro_usd, 0);
        assert_eq!(price.fetch_micro_usd, 4_000);
        // Forcing map to zero browser units is a rule, not a ceiling.
        assert!(!price.clamped);
    }

    #[test]
    fn empty_job_costs_nothing() {
        let price = estimate_job_price(input(BulkJobKind::SearchHydration, 0, 0, 1), JobRates::default());
        assert_eq!(price.total_micro_usd, 0);
        assert!(!price.clamped);
    }

    #[test]
    fn every_estimate_is_labelled_estimated() {
        let price = estimate_job_price(input(BulkJobKind::Batch, 1, 0, 1), JobRates::default());
        assert_eq!(price.source, CostSource::Estimated);
    }

    #[test]
    fn overflow_saturates_instead_of_wrapping() {
        let rates = JobRates {
            static_fetch_micro_usd: u64::MAX,
            browser_second_micro_usd: u64::MAX,
            browser_seconds_per_page: u32::MAX,
        };
        let price = estimate_job_price(input(BulkJobKind::Crawl, u32::MAX, u32::MAX, u32::MAX), rates);
        assert_eq!(price.fetch_micro_usd, u64::MAX);
        assert_eq!(price.browser_micro_usd, u64::MAX);
        assert_eq!(price.total_micro_usd, u64::MAX);
        assert!(price.clamped);
    }
}
