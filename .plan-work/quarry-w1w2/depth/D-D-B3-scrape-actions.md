# Open after D-D-B3-scrape-actions (WS13.5 remainder)

Everything below is still behind `QUARRY_EDGE__SCRAPE_ACTIONS=off`. Nothing here was run against a live Chromium or the
live stack.

## Needs a hand-over to another unit

1. `docs/openapi.yaml` (D-B3-openapi-sdk owns it). `ScrapeResponse` gains an optional `actions` array. Shape, as serialised:
   ```
   actions: [ { index: int, type: string, status: ok|failed|skipped, duration_ms: int,
                error?: { code: string, message: string },
                output?: { kind: screenshot|pdf, content_type: string, bytes: int, width?: int, height?: int,
                           artifact_id?: string   # stored (not ZDR)
                           data_base64?: string } # inline (ZDR, under the shared inline cap); neither = over the cap, left out
              } ]
   ```
   Absent from the body when the request had no actions. `GET /v1/scrape/{id}` returns the same entries (artifact ids only: a
   record is never written under ZDR). New 501 `browser_capability_unavailable` for actions when no browser driver is wired
   (the message now reads "screenshot, pdf and actions need a browser"). Request field `initiated_by` is the tighten-only
   hint already in the contract.
2. SPA types: `data.actions[]` is not in the SPA's scrape response type (gateway forwards the body untouched).

## Not built, by decision of scope

3. Meter metadata `{actions: count, action_types}` (plan routes.rs row). `scrape_usage_metadata` has two branches
   (`usage_attrs` corrected rules vs the legacy map) and a billing test surface; adding a key to one of them is a billing
   contract change, not part of this unit. Count and types only, never selectors or text, when someone takes it.
4. Gateway dispatcher paths (`knowledge.scrape_url`, `knowledge.import_source` in `domains/actions/dispatchers.rs`) send
   no `actions` today, and no verified caller class reaches them (D2-012 open owner decision, see `CallerClass`). The stamp
   is applied in the `/api/v1/knowledge/scrape` route only. When the Model Plane gets a service-principal path to the gateway,
   `stamp_scrape_origin` already makes every scrape from a `CallerClass::ServicePrincipal` carry `initiated_by:"agent"`;
   the dispatchers need the same call when they gain `actions`.
5. `page_renderer` cannot be built in a unit test (needs a NATS JetStream context), so `safe_mode_clears_every_evidence_hop`
   proves Some -> None for ingest, local_index, source_registrar and title_enricher, and covers page_renderer by the
   exhaustive `PageRunner` destructure only. Evidence step: a live actions request must leave the page-image index and the
   CAS untouched (the plan's evidence list, item 8 area).
6. Live Chromium tests were not run (`CHROMIUMOXIDE_TEST=1 ... scrape_actions_live`); this unit's tests use a browser double
   that carries the captures a real fetch would. The runtime-side live tests belong to the WS13.5 runtime slice.

## Findings made on the way

7. FIXED (separate commit): `FallbackDriver` replaced a driver error's `details` with `{fallback_attempts}`, so the typed
   `action_failed` reason and the ordered outcomes the browser driver builds for an `abort` never reached a caller (the 502 body
   had only `fallback_attempts`). The same loss hit any driver-set reason (block reasons). Now the attempts are one more key.
   The file is outside this unit's owned paths; the edit is one helper plus two call sites.
8. FIXED: actions with no browser driver registered ran the page on the default driver without them and answered 200
   (only captures had the 501 check).
9. `fallback_attempts[].error` carries `code: message` of each driver. For action failures the message is fixed text
   ("a scrape action failed"), so no typed text leaks; any driver whose message echoes page text would, but that is the
   existing WS12.S2 error-surface redaction's job, unchanged here.
10. A route-level test with a real service-principal token (403 without `scrape:interact`) is not in
    `tests/scrape_actions.rs`: the edge tests there use the dev bypass (user claims). The principal rules are covered at unit
    level (`actions_tests::service_principal_needs_scrape_interact`). A real-token version belongs beside
    `zdr_claim_floor.rs`'s JWKS fixture.
