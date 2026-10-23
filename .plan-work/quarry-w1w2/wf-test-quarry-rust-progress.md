# wf-test-quarry-rust progress

Unit: test-quarry-rust, wave test phase. Run once and report. No code edits, no commits, no retries.
Worktree: C:/dev/CoresSystem/.claude/worktrees/vigorous-perlman-e7ae5b (branch quarryv2-next)
Workspace: apps/Ingestion Plane/Quarry-v2 (qtest.sh defaults: QROOT and QTARGET quarry-target)

## Pre-flight (2026-10-23)
- Free space on C: 457.82 GB (stop threshold 8 GB): OK
- docker info (timeout 90) answered: server 29.7.2, Docker Desktop: OK
- Packages present as Quarry-v2 workspace members: quarry-core, quarry-transform, quarry-security, quarry-runtime, quarry-edge

## Job (finished)
- One background job through qtest.sh: cargo check --workspace --all-targets, then cargo test --no-fail-fast -p quarry-core -p quarry-transform -p quarry-security -p quarry-runtime -p quarry-edge (default features, no Chromium lane)
- Log: C:/Users/IMAFER~1/AppData/Local/Temp/claude/C--dev-CoresSystem--claude-worktrees-vigorous-perlman-e7ae5b/562f6350-4d29-46dc-a04b-eee466d0c963/scratchpad/WT/test-quarry-rust.log
- End markers: QTEST_DONE (container job, after both cargo commands), JOB_EXIT (host wrapper)
- qwait call 1 reached its 540 s limit with the job still running (test phase had started). qwait call 2 found QTEST_DONE check_rc=0 test_rc=0, then JOB_EXIT rc=0. The job did not time out.

## Result
- cargo check --workspace --all-targets: exit 0, Finished dev profile in 1m 42s, 0 error lines, 19 warning lines in the check step (dead code, unused imports, one deprecated atomic fetch_update call), all in crates/quarry-edge. The test build step printed 19 more warning lines of the same kind (the log has 38 in total).
- cargo test --no-fail-fast -p quarry-core -p quarry-transform -p quarry-security -p quarry-runtime -p quarry-edge: exit 0, Finished test profile in 6m 59s.
- Totals: 56 test result groups, passed 2323, failed 0, ignored 3.
- Per package (sums of the groups below): quarry-core 229, quarry-edge 1060, quarry-runtime 794, quarry-security 31, quarry-transform 209, doc-tests 0 passed.
- Ignored on purpose: redaction_golden redaction_perf_budget (release-only timing); clean_html_cost cleaning_a_five_megabyte_page_costs_this_much_per_option (measurement); doc-test quarry_runtime test_site.rs line 17.
- Binaries that ran 0 tests: quarry_doc_parse (bin, quarry-runtime) and tests/browser_egress_boundary.rs (quarry-runtime). Not investigated.
- Failing tests: none. No timeouts. depth/W1-tests.md was not created because there is nothing to append.

Per test result group (name [binary] passed/failed/ignored), in log order:
1. quarry-core unittests src/lib.rs [quarry_core] 170/0/0
2. quarry-core tests/change_history_proptest.rs 4/0/0
3. quarry-core tests/contracts.rs 30/0/0
4. quarry-core tests/properties.rs 10/0/0
5. quarry-core tests/redaction_golden.rs 15/0/1
6. quarry-edge unittests src/lib.rs [quarry_edge] 394/0/0
7. quarry-edge src/main.rs [quarry_edge_rs] 388/0/0
8. quarry-edge tests/agent_evaluate_denied.rs 6/0/0
9. quarry-edge tests/agent_run_control.rs 33/0/0
10. quarry-edge tests/agent_step_timeout.rs 2/0/0
11. quarry-edge tests/artifacts_fs_list.rs 5/0/0
12. quarry-edge tests/browser_grants_prod.rs 2/0/0
13. quarry-edge tests/cache_policy.rs 10/0/0
14. quarry-edge tests/change_webhook_proptest.rs 4/0/0
15. quarry-edge tests/crawl_limits.rs 5/0/0
16. quarry-edge tests/drain.rs 2/0/0
17. quarry-edge tests/grant_budget_boot.rs 2/0/0
18. quarry-edge tests/grant_budget_claim.rs 8/0/0
19. quarry-edge tests/job_admission.rs 2/0/0
20. quarry-edge tests/log_field_discipline.rs 22/0/0
21. quarry-edge tests/metrics_recorder.rs 7/0/0
22. quarry-edge tests/profile_roundtrip.rs 11/0/0
23. quarry-edge tests/properties.rs 6/0/0
24. quarry-edge tests/ready_browser.rs 6/0/0
25. quarry-edge tests/run_sweeper_integration.rs 25/0/0
26. quarry-edge tests/scrape_records_durable.rs 4/0/0
27. quarry-edge tests/sse_stream.rs 1/0/0
28. quarry-edge tests/start_run_grants.rs 7/0/0
29. quarry-edge tests/zdr_behavior.rs 4/0/0
30. quarry-edge tests/zdr_claim_floor.rs 24/0/0
31. quarry-edge tests/zdr_feature_matrix.rs 13/0/0
32. quarry-edge tests/zdr_gap_rows.rs 9/0/0
33. quarry-edge tests/zdr_log_capture.rs 12/0/0
34. quarry-edge tests/zdr_posture_stamp.rs 13/0/0
35. quarry-edge tests/zdr_refusal_modes.rs 8/0/0
36. quarry-edge tests/zdr_tombstone.rs 25/0/0
37. quarry-runtime unittests src/lib.rs [quarry_runtime] 776/0/0
38. quarry-runtime src/doc_parse_main.rs [quarry_doc_parse] 0/0/0
39. quarry-runtime tests/browser_egress_boundary.rs 0/0/0
40. quarry-runtime tests/doc_parse_sandbox.rs 10/0/0
41. quarry-runtime tests/dom_summary_proptest.rs 4/0/0
42. quarry-runtime tests/integration_runtime.rs 3/0/0
43. quarry-runtime tests/publisher_integration.rs 1/0/0
44. quarry-security unittests src/lib.rs 20/0/0
45. quarry-security tests/security.rs 11/0/0
46. quarry-transform unittests src/lib.rs 156/0/0
47. quarry-transform tests/clean_html.rs 16/0/0
48. quarry-transform tests/clean_html_cost.rs 0/0/1
49. quarry-transform tests/clean_html_selectors.rs 16/0/0
50. quarry-transform tests/markdown_blowup.rs 10/0/0
51. quarry-transform tests/transform.rs 11/0/0
52. doc-tests quarry_core 0/0/0
53. doc-tests quarry_edge 0/0/0
54. doc-tests quarry_runtime 0/0/1
55. doc-tests quarry_security 0/0/0
56. doc-tests quarry_transform 0/0/0

Log: C:/Users/IMAFER~1/AppData/Local/Temp/claude/C--dev-CoresSystem--claude-worktrees-vigorous-perlman-e7ae5b/562f6350-4d29-46dc-a04b-eee466d0c963/scratchpad/WT/test-quarry-rust.log (2889 lines)

## Next
- Nothing left in this unit. Return the result. No commits were made and no code was edited.
