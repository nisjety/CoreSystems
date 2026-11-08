# F3 change log (review K3, code-fidelity skeptic, Wave 2)

Plan: `docs/plans/space-hybrid-build-plan-2026-10-14.md`. Plan line numbers are those at the time of the edit and drift by a few lines.

| finding id | status | what changed (plan line now) | note |
|---|---|---|---|
| K3-01 | ALREADY | WS10 bullet (1915) already says only `consumed_at`, `continuation_mode`, `ConsumeApproval` are built and `scope`, `cause` are reserved; ruling (3) (1918) says the WS10 slices that read them wait for (b)'s PR; Appendix A (5105) records it | applied earlier under K1-08 |
| K3-02 | APPLIED | execution-core test command (1982): `cargo test -p execution-core -- approval_gate permission approval_delivery_worker` | Grep found no other multi-filter cargo command |
| K3-03 | APPLIED | WS14 verified-start bullet (2391): share actions are not in `MODEL_EXECUTABLE_ACTION_IDS` (`model-eligibility.ts:126`), hole restated as user-reachable un-owned grant write; S4 row (2429) and registry bullet (2445): owner plane `data`, stay out of the model-executable list, negative assertion in `catalog-manifest.test.ts`; gateway test line (2461) and DoD step 5 (2469) use `isModelExecutableAction(...)` is false; risk (2473) and Q5 (2491) reworded; M2-S7 evidence (4710) | Grep finds no remaining "model actor" or "latent" |
| K3-04 | APPLIED | WS14 typed-refusal bullet (2442): `zdr_durable_mutation_forbidden` 412 with no `feature` field, gateway-local `details.surface:"room_files"`, catalogue label only via WS06; WS12 DM routes (2210): "(no `feature` field)" | `zdr_refusal.rs:190-196` `takes_feature: false` consistent with the finding |
| K3-05 | APPLIED | WS08-S2 grpc row (1717) and 4.4.1 row 12 (3770): time order WS01-S1 > WS01-S3 > WS08-S2 > WS12-S4 > WS15-S9, call-site hunks only, later slices rebase | |
| K3-06 | PARTIAL | Row 16 (3773) now lists WS09-S1 (1), counts the whole `sse.rs` list once for integrator approval and fixes the order inside `load_recent_thread_messages` (WS08-S3 first, WS12-S4 second); WS08 conventions line (1682) names WS09-S1 and WS12-S4; WS09-S3 row (1833) and risk row (1891) say four hook lines; WS09 question 4 (1904) now "four"; new WS12 open question 6 (2269) | the "K1 ruling 4" part (row 16 closed list) was already applied by F1; this adds the missing hooks |
| K3-07 | APPLIED | M2.2 composition (3404) adds WS15-S5 (pulled by R7); M2.3 composition (3405) and T6 (3820) exclude S5; slice row (3596) says "M2.2 (pulled by R7)"; WS15 lane line (2495) says S5 ships in M2.2 | T5 (3818) and R7 already carried it. Pending recount note on both M2.2 and M2.3 effort cells (WS15-S5 0.8-1.2 ew); sections 1.4, 4.1, 4.2, 4.7 not recomputed |
| K3-08 | ALREADY | M2.1 gate (3403) already reads "THREAD grant and revoke are proven after (b) lands mp-WS13-S6, criterion 10b"; criterion 10b conditional (187); WS10 Q4 answered (2012) | applied earlier by the review ruling 8 |
| K3-09 | APPLIED | One sentence on live-tree versus `model-plane` line offsets and "anchor by function name, re-grep at `M0-BASE`" added to the WS08 conventions (1682) and the WS12 seam mapping (2134) | |
| K3-10 | APPLIED | Section 5 row WS10-S5a (3546): "tool policy store (Space ceiling, agent layer)" | |
| K3-11 | APPLIED | WS09-S3 contract-file path (1833) and test command (1875): `apps/Model Plane/scripts/tests/room-run-frames-contract.test.sh`, run from the repo root; WS10 (1981) and WS14 (2458) commands say "from the repo root" for `scripts/...` | |
| K3-12 | APPLIED | WS12 Depends-on (2138) adds `M0-T0`; DoD seed sentence (2241) cites M0-T0 | |
| K3-13 | APPLIED | WS09 header (1794): S7 needs `P-UC1`, WS03-S1 first by R3 (soft in the graph) | |
| K3-14 | APPLIED | WS09-S2 row (1829) and flag table row (1864): flags live in `SpaceFeatureFlags` (P-GW1), parsed once in `build_state` | |
| K3-15 | APPLIED | WS11 bullet (2020) names `verevon-gateway` and the `validateDelegationKeys` rule (`config.go:112`, verified in the live tree); WS11-S9 row (2066) adds extend-the-switch and key-map instructions | WS08-S7 (the `model-gateway` principal) was removed by F1, so only `space-attention` is covered |
| K3-16 | APPLIED | WS10-S3 row (1950) and risk row (1991): default to `ClaimOnceApproval` unless a `model-plane` test shows `ConsumeApproval` accepts `continuation_mode = ''` and binds `action_fingerprint`; never extend (b)'s message; bullet added to Known unverified items (5109) | UNVERIFIED finding; safest reading applied as a default, plan text otherwise unchanged |
| K3-17 | APPLIED | WS08-S2 terminalization row (1718): extend `ExistingManagedStart` and its SELECT (`:741`) with `trigger_message_id`, `agent_subject_id` from `runs`; ZDR never reaches it | |

Stale-copy Grep: no "WS12-S4 > WS15-S9 > WS08-S2", no "three hook", no "model actor", no "org floor" in the WS10-S5a row, WS15 S1-S9 composition corrected in M2.3 and T6.

Pending recount: M2.2 (33.0-48.4 ew) and M2.3 (72.6-109.3 ew) totals, and the per-milestone lines of 1.4, 4.1, 4.2, 4.7, are not adjusted for moving WS15-S5 (0.8-1.2 ew) into M2.2; noted beside both cells.

<!-- FIX COMPLETE -->
