# D2-B-x10-robots: depth findings left open (decision 3.1, X-10)

Each item is something this unit found and did NOT fix, with the reason and who decides.

1. **/v1/map decisions are not counted.** `map_routes.rs` consults the gate only under enforce, and calls `rules_for_origin` (the state), not `record()`. A map refusal (unreachable robots.txt under disallow, 403) therefore does not appear in `quarry_robots_decisions_total`. Under observe, map does not consult the gate at all, so the soak gets no map data. The decision accepted this ("/v1/map is checked separately on the e2e enforce leg"). Owner: decide whether map counts as one decision per origin.

2. **Distinct origins are not exported.** The owner's volume stop rule is "enough crawls across enough distinct sites". The counters carry `mode` and `outcome` only, no host. The per-decision ratio (the go criterion) is readable; the distinct-site count is not. The decision named an origin-level signal as the alternative. Not built: it would add a counter per robots.txt fetch (found, absent, unreachable, closed reason classes). Owner: add it, or measure volume by crawl jobs.

3. **The metadata-redirect test accepts either answer.** `a_robots_txt_redirect_to_the_metadata_address_counts_as_unreachable` (crawl_scope_robots.rs) asserts `200 || 403`. The fixture driver follows the hop to a host it does not serve (404, which reads as absent), so the test does not prove the refusal. The real refusal is checked in `quarry-runtime/tests/robots_gate_ssrf.rs` against the StaticDriver. Not strengthened here (it would need a fixture that refuses hops).

4. **The e2e redirect leg cannot see a refused hop.** The fixture host's log shows the robots.txt request, not that the edge refused the metadata hop. The stack session needs another observable: the `page_blocked` event with `stage` and `reason`, or the crawl's denial. The unreachable leg is observable (two robots.txt fetches, no page fetch).

5. **Scheduled crawls with no stored scope bypass the gate** (D2-318, D2-161 part). Not changed. This belongs to X-9, and scheduled jobs stay off.

6. **A 5xx robots.txt stops an origin for 5 minutes** under enforce with the disallow policy (decision 3.1, open risk). Known and accepted by the owner's yes; no change.

7. **Product token open (F-t2-067).** The gate answers for `quarrybot` and `quarry`; the advertised user agents differ (`Quarry/2.0`, `QuarryBot/1.0`). Not decided.

8. **A cancelled crawl does not carry the robots count.** The durable and legacy crawl `run_cancelled` payloads have no `pages_blocked` (existing shape), so `pages_blocked_robots` appears only on `run_completed`. The composer row therefore does not show on a cancelled crawl. The batch cancel payload does carry it. Owner: add the scope summary to the crawl cancel payload if the row must show on cancel.

9. **A cross-origin redirect is two decisions for one page** (request and final_url). The counters count decisions, so the ratio is per decision, as the criterion says. Documented in RUNBOOKS.

10. **compose not validated by `docker compose config`.** The two new env lines copy the neighbours' pattern exactly, but this worktree has no env files, so the file was not rendered. The stack session should render it (`docker compose config` with the usual env files) before the flip.

11. **e2e legs never run on a stack.** observe, enforce (with the map check), unreachable and redirect were syntax-checked and their input checks run offline; they need the stack session (DEV_BEARER, docker). The unreachable and redirect legs also need fixture hosts the overlay does not serve yet.

12. **Stage value.** A cache-hit decision reuses `stage: "request"`. A consumer that wants to tell a cached refusal from a fetched one cannot, from the event alone. Left as is to keep the vocabulary closed.

13. **A cache hit can now fetch robots.txt, under observe too.** Before D2-161 a page served from the page cache made no robots.txt request. Now its origin is decided like a fetched page, so the first cached page per origin per 24 h (rules) or 5 min (unreachable) fetches robots.txt. That is extra outbound requests in observe. It is what the soak needs (the cached pages must be counted), but it is a change to egress, and the owner should know it. Under enforce, a cached page on an origin whose robots.txt disallows it, or cannot be read under the disallow policy, is now refused (403) instead of served.
