# INNH depth findings, lane A (scrape / extract)

One line per finding. Things worth doing later; none is needed to close the item's gap.

INNH-1 | The privacy skip happens after the page fetch (the scrape is still served); a request that is both AI-formatted and credential_or_secret could skip the model lane checks (501s) as ZDR does, but today a deployment without a model still answers 501 before the fetch for such a request. Decide whether the privacy block should behave like ZDR there.
INNH-1 | The ZDR matrix rows for scrape and extract do not name the PRIVACY_POLICY skip (it adds no persistence or new content surface, so the golden was not regenerated); add it to the basis text next time the golden is regenerated.
INNH-1 | /v1/extract reports a privacy-block refusal as a per-source error string starting PRIVACY_POLICY:, like ZDR_UNAVAILABLE:, not as a structured code; a typed code on ExtractItem would be cleaner for the UI.
INNH-7 | max_age is scrape-only: crawl/batch/run_page keep prev_fingerprint. A per-job scrape_options.max_age (INNH-6) would reuse PageCache::result_key and scrape_cache::rehydrate unchanged; needs the orchestrator to carry the option and a decision on billing for hits.
INNH-7 | A hit is not metered as scrape.page (same as the existing page-cache hit). Decide with the Control Plane owner whether a cache hit bills, is free, or bills a smaller unit; the response already says cache_state hit.
INNH-7 | Redirects: the key uses the URL as requested, not the final URL, so a.com and a.com/ that redirect to one page are two entries. Aliasing under the final URL (a second key, same output) would raise the hit rate.
INNH-7 | Browser-served results stay uncached in v1 (also a static request that escalated to the browser). A browser entry would need the render hints and wait options in the options key.
INNH-7 | The write happens for every cacheable success while the flag is on, even for requests that never send max_age, so the first max_age request can hit. If Redis memory matters, switch to write-on-demand (write only when the request carries max_age) and accept a cold first call.
INNH-7 | Option explosion: each distinct option set is its own entry (TTL 3600 s). If a caller varies include_tags per request, memory grows; a per-org entry cap or a shorter TTL for option-bearing keys is the lever.
INNH-7 | stale_while_revalidate_s, min_age and a store=false switch (Firecrawl) are not built; CachePolicy.vary_on is still read nowhere.
INNH-7 | quarry-v2-ci.yml runs cache_policy.rs in the Redis step; scrape_max_age.rs needs the same step (file is outside Quarry-v2; left to the owner or CI session).
INNH-7 | fingerprint() in cache.rs is still dead code (#[allow(dead_code)]); the URL key supersedes it. Delete it or wire it.
INNH-7 | Under QUARRY_CHALLENGE_ENFORCE=log (observe mode) a challenge page is let through the gate and would be written to the URL cache; add a challenge signal check to scrape_cache::is_storable if that mode is ever run with the max_age switch on.
