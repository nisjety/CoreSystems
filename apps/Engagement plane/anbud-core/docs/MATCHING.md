# Pipeline matching

Anbud Core uses the pure deterministic `internal/match` rules engine, not an LLM,
embeddings, or external reranking. Eligibility (deadline, exclusion, company status,
qualification/document evidence) is independent of relevance (CPV, NUTS, value,
manual keywords and optional company SEO terms). Missing evidence stays unknown.

The matching feed reads every latest live contract-notice identity from the prepared
projection and loads full matching facts only for notices with an unpassed deadline.
It deduplicates cross-source twins and settles procedure amendments before selecting
active opportunities. A newer closed amendment therefore cannot revive an older open
notice. Undated notices are counted separately rather than assumed active. The old
`scan` request field remains accepted for compatibility but does not truncate this
feed. The response reports `coverage: all_active` and `active_assessed`. Results are
still a ranked top-N page; that display limit does not limit assessment coverage.

Prepared matching facts are versioned separately from request/user context. They
contain only notice facts, no profiles, user fields, contact data or winner identities.
Both list and matching facts are erased by the existing source-linked projection
redaction transaction. Identity-only inactive versions are hydrated only when needed
for a selected amendment diff. No diff is computed against deliberately omitted text.

The separate `tracked_results` response reads the organisation's persisted review,
working, sent and hidden stages, through workspace scope/RLS plus an explicit org
predicate. These tenders survive expiry, mismatches and the inbox limit. Current
warnings remain visible; retention never turns a failed assessment into a pass.
Missing/redacted sources yield only an identifier/unavailable marker. A
`tracked_only` request can read saved work without a matching profile; it returns
unknown assessments. Organisation identity always comes from authenticated headers,
not the JSON body.

The client reads current SEO query aggregates from the active organisation's verified
primary website using Visibility Core's existing authorised read API. Only connected,
non-expired evidence (at most 30 days old) contributes up to 50 bounded terms. It
also reads the current user's position and department through Control's existing
profile API, keeping at most 20 professional terms and excluding generic job labels.
Names, contact details, location and protected personal attributes are not used.
No context is persisted in the public projection or browser storage. Unavailable
context sources contribute no signal. Loads are bounded and cancelled on navigation;
answers are discarded if the user or organisation changes.

Those terms travel as advisory search preferences (`context.company_keywords`,
`context.user_keywords`), not eligibility evidence or authority claims. SEO terms can
supply a company-scope signal. Professional terms only break relevance ties within
the same eligibility and company-fit band; they cannot admit a tender by themselves.
Matched terms are named in fit checks (`company_seo`, `user_focus`). Short manual
keywords and all automatic terms use word boundaries to avoid IT/activities-style
substring false positives. No semantic translation or qualification is inferred.

Visible pipeline pages refresh every minute, after a saved stage change, and when
opened. Existing company profiles remain explicit and are not silently overwritten
by SEO or user preferences. The shared company pipeline stays shared while ranking
can differ by the requesting user's professional focus.

Pipeline cards preserve server relevance order by default; choosing deadline/title
sorting is an explicit presentation override. Current deadline/eligibility warnings
remain separate from relevance ordering. Searches without an explicit profile can
load tracked work, but never manufacture an automatic inbox from work-role terms.

Local verification on 2026-10-29: 6,218 stored contract-notice identities, 3,236
current active opportunities after lifecycle settlement. With legacy `scan: 1`,
coverage remained 3,236. Sample complete responses took 566–694 ms; display remained
limited to the ranked first 50 while all active opportunities were assessed. These
are local samples, not a production percentile claim. UI checks used fixtures for
identity, SEO, and stage persistence with real active-matching responses; isolated
PostgreSQL tests exercised actual workspace persistence, tenant isolation, expiry,
source erasure and projection refresh.


## Pipeline pagination

The UI opts into `paginate: true`. The matching inbox pages through all matching
active opportunities, 50 at a time. Search, filters and sorting run over the complete
inbox before paging. Saved review/working/sent/hidden tenders are returned separately
on every page and do not consume inbox slots.

Opaque cursors reference immutable in-memory snapshots bound to the authenticated
organisation, actor, profile, search, sort and page size. Advisory SEO/professional
context is frozen from the first request. Snapshots expire within ten minutes, at
the next deadline or Oslo midnight, and invalidate when source generation, uploaded
evidence, registry status or retained membership changes. The UI restarts on page one
with an explanation when a cursor expires. Storage is bounded to eight snapshots and
32 MiB per process; there is no persistent matching cache.

The UI keeps at most three pages in memory, prefetches the next page and revalidates
foreground navigation. Changing filters/profile or organisation clears cached pages.
Local sample on 2026-10-29: 302 unique matches reached across seven pages
(50/50/50/50/50/50/2), with no duplicates or omissions. First request: 529 ms;
subsequent requests: 1–3 ms. These are local samples, not production percentiles.
