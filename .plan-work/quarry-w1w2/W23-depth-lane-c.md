# W23 depth findings, lane C (not fixed; one line each)

Format: `unit | finding`. Wave first, depth later.

- W2-U30-33a | `include_answer` on `POST /v1/search` (`search_routes.rs` ~894) calls the ungated `AnswerPipeline::answer`, so its page fetches skip the web policy. `answer_routes::url_gate` (make it `pub(crate)`) plus `answer_gated` closes it; the search route already holds a policy `decision`. Outside this unit's file scope.
- W2-U30-33a | The Data Plane / local-index search tiers return result URLs with chunk text as the snippet. A policy-denied URL from those tiers is skipped like a web one (no snippet fallback), which also hides the chunk; whether a private-corpus hit should be gated by a web-domain rule at all is a product decision.
- W2-U30-33a | `gate_url` counts a denial per URL per request (`record_denial`), so a query whose top-K are mostly denied hosts bumps the denial counter K times; a per-request aggregate (like `gate_urls`' `denied_count`) would be quieter.
- W2-U30-33a | The streaming answer cache key is org + query only (`answer:stream:v1`); it does not include the policy decision, so an answer cached under a permissive policy is replayed after the org tightens its policy. Same shape as the scrape cache's `is_default` rule; either key the entry by a policy fingerprint or skip the cache when the decision is not the default.
- W2-U30-33a | The RunPolicy `policy_fp` is not computed on the answer routes at all; "policy_fp unchanged" holds trivially. If answers should be reproducible per policy, the response could echo a fingerprint.
