# Model Plane: owner decisions for audit phase 2 (2026-11-20)

The 83 decision items of the phase 2 inventory (`model-plane-audit-phase2-inventory-2026-11-20.md`, section 1) collapse into the 60 questions below, because several items ask the same thing. Each question has a recommended default. Answer by number, or say "defaults".

## A. Spaces and personal routes
1. **Space turns on personal routes** (OD-40, R-spa-02, R-spa-03, R-spa-10). Default: refuse. A Space turn never uses a member's own subscription or gateway: model-gateway refuses with `scope_not_available_in_space`, the SPA and gateway switches stay equal (a registry cross-check test), cancel stays allowed, approve and resume are refused.

## B. Org command rules (`command_prefix`)
2. **Programs that run a command from their own options** (R-ec-18: rsync, scp, sftp, zip, make, vim, ex, git -c). Default: withhold rsync, scp, sftp, zip, make, gmake, vim, vi, ex, nvim, view always; git only with `-c`, `--upload-pack` or an `alias.` word.
3. **Code in files, modules or stdin** (R-ec-20: `python x.py`, `node app.js`, `awk -f`). Default: out of scope, written down in `execution-core.md`.
4. **Runtime data evaluated as shell** (R-ec-05: `${x@P}`, zsh `(e)`, arithmetic on a variable). Default: withhold those forms.
5. **The empty command_prefix rule matches everything** (R-ec-26). Default: keep (fail closed); list ENV, ZDOTDIR, PS4, BASH_FUNC_* in the docs only.
6. **setarch under other architecture names** (R-ec-24). Default: keep the explicit list.

## C. Security switches and defaults
7. **AUTHCTX_ENFORCE unset** (R-swm-09). Default: Rust services enforce unless `ALLOW_INSECURE_DEV_DEFAULTS` is exactly `1`; observe mode drops a mismatched X-Org-ID.
8. **Quarry edge switches** (R-ing-08: STEP_EXPECT, PROCEDURE_CANONICAL, HUMAN_BROWSE, ATTACH). Default: tighten all four to exact `true` in one commit, after you check deployed env values.
9. **BILLING_QUARRY_PLAN_CAP_DEFAULTS** (R-ctl-01). Default: exact `true`, with a registry row.
10. **Three execution-core switches outside the exact-literal rule** (R-swx-03). Default: keep all three as documented exceptions.
11. **CSP_REPORT_LOG_HOSTS** (R-bff-03). Default: keep as the documented dev-only exception.
12. **Cased `Off` in mode switches** (R-sws-17). Default: any-case `off` reads Off everywhere (write policy, skill lint, sweeper); report and enforce stay exact.
13. **Dev defaults for the Space approval gate and browser human browse** (R-reg-12). Default: dev off for both, registry rows aligned.
14. **TTS residency lines** (R-reg-04). Default: move them to the inference-core block (default false).

## D. Spaces, sessions, deletion
15. **Edited and deleted messages in headlines and search** (R-sws-02). Default: use the fold everywhere, behind a new default-off switch.
16. **Approval floor gap** (R-sws-05). Default: accept (fail closed), count it, stamp approvals only if the counter is non-zero.
17. **Thread cursor metadata** (R-sws-09). Default: accept (counts and a role, no content), document it.
18. **Titles written from pre-join turns** (R-sws-10). Default: hide them for a reader with a floor.
19. **Two deletion requests for one Space** (R-app-02). Default: dedupe in Application only, return the existing receipt.
20. **Parked delivery on an "unknown" or "failed" answer** (R-app-06). Default: close only on a definitive answer; keep it parked otherwise.
21. **Attempt counting for deletion deliveries** (R-app-07). Default: count consecutive throws only, park at 12, reset on re-arm.
22. **Revoked agent's memory erase after a hold or outage** (X-revoked-agent-memory-erase-retry). Default: session-core writes one ids-only pending-erase row and its sweeper retries.
23. **Hold answers without a retryable flag** (R-spa-13). Default: keep the fail-closed 503 and unify both surfaces on `erasure_hold_check_unavailable`.
24. **Reactions UI vs upstream switches** (R-bff-04). Default: key it off the flag Convex already returns on the thread read.
25. **Redirect onto an unreported denied host** (R-mg-05). Default: keep the V3b rule, add a counter, revisit with the count.

## E. Confirmations and cleanup
26. **Second Ingestion tree `apps/Frontend Plane/Ingestion Plane`** (C-19, R-ing-02). Default: delete it with one `git rm -r` commit, after updating `.gitleaks.toml` and the pre-commit hook.
27. **FU-30 assumptions** (C-23): cost_basis in the ZDR spend allowlist, and subscription/metered rows still reach billing. Default: confirm both; this gates `MODEL_GATEWAY_ZDR_LEDGER_ROWS`.
28. **User erasure and approval rows** (C-24c). Default: keep the rows but clear the person-written columns, for both user and anonymize.
29. **Meter council, review and side chat into the cost ledger** (X-usage-envelope-metering). Default: yes, one content-free row per call, budget checked first.
30. **OD-01..OD-26 assumed defaults** (incl. OD-25: anonymize removes the person's source link from Space memory rows; the erasure audit row keeps the person's id). Default: confirm all.
31. **OD-64 rollout, OD-65, OD-66.** Default: confirm as recorded.

## F. Deferred audit items (default: wait or no)
32. **Telemetry export and the `error` field** (F-t3-004, F-t3-062 registry). Default: build no export; keep the allow-list off until you decide `error`.
33. **Browser login hand-off** (F-t3-008-s9). Default: wait for Quarry redaction.
34. **Sandbox backend and desktop spike** (F-t3-009-s7/s8, OD-45). Default: only when a second real backend exists; no code now.
35. **Durable reference upload** (F-t3-017-s8). Default: stay deferred.
36. **Point-and-comment steering and annotation** (F-t3-026, F-t3-031). Default: wait.
37. **Mid-run steering of deep research** (F-t3-027). Default: do not build.
38. **Pre-run cost upper bound on the plan card** (F-t3-028-s11). Default: wait for your wording under DEC-18.
39. **Unattended runs with the user's authority** (F-t3-037-s7, F-t3-064, OD-49, OD-63-ws20s9). Default: no; keep DEC-02, Ask-only API, read-only unattended goals.
40. **Experiment ids and user-path budgets** (F-t3-040). Default: defer.
41. **Meeting pull and video models** (F-t3-041). Default: no; frozen contract untouched.
42. **Credential-injecting proxy** (F-t3-043-c). Default: ratify O1-O3 as written, O4 as a rate-limited log; build later behind `SANDBOX_CREDENTIAL_INJECTION`.
43. **Lifecycle note store and connector incident ring** (F-t3-049-s8/s9). Default: no store; hand the ring to WS23.
44. **Regime tags vocabulary** (F-t3-062). Default: wait for counsel.
45. **Sharing beyond org-only snapshots** (F-t3-062-share-links, OD-47). Default: keep org-only.
46. **Speech-to-text, F-t3-084/085, H11 and apply_patch** (F-t3-082/084/085/088, OD-41..43). Default: wait for demand; keep H11 in wave 6.
47. **Channel Plane ADR first** (OD-44). Default: yes, write the ADR, build nothing before.
48. **Continuous browser video** (OD-46). Default: keep the screenshot filmstrip.
49. **Binary artifact home and image signing** (OD-48). Default: decide the home first, no C2PA until then.
50. **Cloud-role credentials as BYO** (OD-50). Default: last.
51. **Trigger sources beyond webhooks** (OD-51). Default: wait for those contracts.

## G. Contracts and deferred slices
52. **integration-corev2 verifies the Auth Core retention decision** (OD-52). Default: yes, behind a default-off switch.
53. **Audit row for unauthenticated requests** (OD-53). Default: rate-limited log only.
54. **The custom_rest section W4-CC-6 added** (OD-54). Default: keep it, review it, no further edits.
55. **Optional `action_id` on Quarry timeline entries** (OD-55). Default: send the contract request.
56. **Schedule definition snapshot** (OD-57). Default: snapshot at creation, erased with the schedule.
57. **Data Plane request for the embedding ZDR field** (OD-60). Default: yes, send it.
58. **Waiver contract version readiness check** (OD-61). Default: yes, an S unit adds the health field and a read-only script.
59. **The ten deferred slices** (OD-63, ws19s10, ws23s9). Default: keep all deferred; decks stay Ask-only, calendar writes wait.
60. **Deferred audit parts with no decision row** (F-t3-048, 053, 058, 059). Default: keep deferred, record each reason in one OD row.
