# C9 verify: EP-5 S1 (pptx skill + manifest v1 + build_deck.py)

Result: PASS with live-proof gaps.

- Manifest versioned: DeckManifest.version = MANIFEST_VERSION (1); validate() refuses any other; canonical JSON starts `{"version":1,"deck_id":`. mp-contracts `cargo test deck`: 19 passed.
- Per-slide validation: SlideInput/Slide validate() return field-path errors (over-cap, unknown layout, missing anchor); builder report gives ok/warn/error per slide. Covered by those 19 tests.
- ZDR refuse: deck_tool refuses when turn.zdr; deck_templates route refuses ZDR tokens. model-gateway `deck`: 48 passed; `--test zdr_posture_matrix`: 10 passed.
- build_deck.py: `python -m unittest test_build_deck` OK, 26 run, 5 SKIPPED (python-pptx not installed, so build/read-back/picture/theme paths unexecuted).

## D3 findings
- D3-new-1 (P1, test-gap): 5 builder tests skipped here (no python-pptx 1.0.2); same as D3-290. Not proven.
- D3-new-2 (P1, risk): `cargo test -p model-gateway zdr` has 3 failures in zdr_posture::tests: every_non_get_route_is_classified (browser key-grants, threads seen, threads workspace delete have no row in space-surface.v1.json), no_raw_retention_claim_is_read_outside_the_named_inventory (assistant_navigation.rs, auth.rs, http_routes.rs deck_templates_listing and mcp_app_resource read claims.zdr raw), every_inference_call_site_is_named (assistant_navigation::classify). Not S1 code; deck_templates_listing belongs to S7. Needs owners before merge.
- D3-new-3 (P2): D3-291..293 (fit estimate unvalidated, live proof deferred, keep BUILTIN_OFFICE_SKILLS_ENABLED off) still open.
