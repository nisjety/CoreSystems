# S-WS3.K hand-over

Done and committed on quarryv2-next (local only): 1f13a635, d5a160eb, 4f2b4442. Design: wf-S-WS3.K-design.md. Ledger text: wf-S-WS3.K-ledger.md. Progress file: wf-S-WS3.K-progress.md (superseded by this note).

Left for a human or a later round:
1. Flag soak and rollout (RUNBOOKS, "Trusted input for ref actions": live lane green two weeks, then one edge, watch `quarry.browser.actionability_rejected` per reason).
2. Evidence run on a rebuilt edge with `QUARRY_BROWSER_TRUSTED_INPUT=on` (modal over a link on a public page; 409 target_covered, close ref 200, link 200 with a changed url). Not possible here.
3. Sibling (b): planner sentence for `target_covered` and friends, closed `browser.act` schema. No Model Plane file was touched.
4. WS3.L items. WS3.G is next in the chromiumoxide.rs queue.

Traps: rustfmt is on the host, not in the container; chromiumoxide.rs and deadlines.rs are not rustfmt-clean at HEAD, so only own hunks were formatted. Live tests are in `src/trusted_input_live.rs`, `include!`d at the end of `mod tests` in chromiumoxide.rs (one line there). `sdks/generate.sh` ran under with-lock (image openapitools v7.22.0 was local); only two TS docs files are tracked output. Files outside my paths that show as modified in git status (quarry-core Cargo.toml and lib.rs, .github/workflows/contract-goldens.yml, the WS5.G files) are other agents' and were not touched.
