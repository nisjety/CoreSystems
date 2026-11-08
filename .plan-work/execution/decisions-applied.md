# Decisions applied

The owner said: run all the slices, decide everything, use the plan's recommended defaults. This file lists what was applied by default and the few points that still need the owner. The full default text for every decision a slice names is in `queue.json` under `decision_defaults_applied`.

## Applied by default (no owner input needed)

- **Plan decisions DEC-01 to DEC-40 and Q-7a1/Q-7a2 marked agent-default in `decisions.json`:** applied as written. The main ones are:
  - DEC-01/03: execution-core hosts the loop and session-core holds the fold.
  - DEC-04: approving a plan starts a new execute run.
  - DEC-05: ask_user has its own `run_questions` store.
  - DEC-06: Space runs get a typed refusal.
  - DEC-07: NATS request/reply. If the bus is down the answer is a 503, and the editable queue stays client-only.
  - DEC-08: approval scope is `ApprovalScope {ONCE, THREAD, RUN}`, and "always" is allowed only for mcp_tool.
  - DEC-09: step-up is a Control-minted `step_up` claim.
  - DEC-10: Do is off in code and compose and on in the proof stack.
  - DEC-12: the class ceiling is set per capability, the per-tool rows hold the truth, and MCP annotations are untrusted.
  - DEC-13: org-core stores model access and inference-core enforces it.
  - DEC-14/15: capability-core owns org agent policy. Strictest rule wins and hook rules fail closed behind their switch.
  - DEC-16: unattended runs use a Control-signed stored-consent contract.
  - DEC-18: billing is recorded behind a default-off flag.
  - DEC-22: there is no SandboxBackend trait until a second backend is built.
  - DEC-29: messages form a general tree, history is paged with a `before_message_id` cursor, and search uses FTS `simple`.
  - DEC-30: memory preferences live in session-core, and learned skills are disabled proposals.
  - DEC-31: the content-safety default mode is `frame`.
  - DEC-32: waived runs refuse every new store.
  - DEC-35: the audit chain is signed with an Ed25519 key in env plus an external witness, and SIEM is gated by a config flag.
  - DEC-36: Do-run effort stays 0 in W1, the per-level allowance is raised, the Codex broker is not widened, and the effort cap is a seam (clamp, not refuse).
  - DEC-37: caps stay at 1 MB and are declared by the server.
  - DEC-38: notification-core uses a native adapter and push is off for ZDR.
  - DEC-39: the integrator numbers proto tags and migrations. The lead holds the integrator role.
- **Owner sign-offs named in the slice rows** (ledger acknowledgements, "owner approval to flip on the proof stack", Control Plane principal reviews, the K3-14 addendum to `actions-surface-operations.md`, the WS07-S10 $0.50 eval budget, the CI run cost, `MCP_DO_TOOLS=read` on the proof stack): treated as decided by the owner's blanket instruction. Every switch still ships default-off, and the lead turns it on in the proof stack only.
- **Optional slices built because their gating default says build:**
  - WS07-S6, per-thread memory off (built unless declined).
  - WS11-S7, the effort-clamp seam (DEC-36, "seam now", cap None until WS10).
  - WS13-S5, step-up (DEC-09 (a)).
  - WS13-S6 and WS13-S7, approval scope and the "always" rule (DEC-08).
  - WS14-S5, same-run resume (DEC-03/05).
- **Optional slices deferred because their default says not now:** WS02-S12, WS04-S8, WS08-S8, WS09-S4/S6/S7/S8/S9/S10 (they depend on the WS09-S1 numbers, so they are re-evaluated after it), WS11-S8, WS11-S9, WS14-S6, WS15-S8, WS15-S9, WS19-S10, WS20-S9, WS21-S9, WS22-S7, WS22-S8, WS23-S9 and WS27-S11. The reason for each is in `queue.json` `remaining`.
- **Live proofs that need "a live inference route":** these use a route the dev stack already has (it ran the WS03 live proofs). Only named routes the stack does not have are treated as blocked-external: gpt-5-mini, Foundry claude-opus-4-8, vision deployments, staging Azure, and the Codex subscription.
- **Dependency resolution:**
  - Workstream-level dependencies point to the slice that provides the needed seam. For example, WS08 resolves to WS08-S2, WS26 to WS26-S2, WS12 to WS12-S4, WS11 to WS11-S5 and WS07 for skills to WS07-S7. Each record's `dependency_notes` says which resolution it used.
  - A dependency on a blocked slice points to its `.code` half.
  - WS13-S1 now waits for WS13-S0, because S0 pins the behaviour that S1 changes.
- **Cycles broken:**
  - WS02-S6 and WS02-S7 depended on each other. S6 goes first (train T3 and the agent.rs order).
  - WS09-S5 and WS12-S12 depended on each other. S12 follows S5, because its job skips until S5 exists.
  - WS01-S0 → WS12-S2 → WS12-S1 → WS01-S0 contains only landed work, so the old edge was dropped.
  - WS17-S8 to WS09-S8 is now a soft rebase, not a dependency (graph cycle_breakers).
  - The WS14-S7 and WS21-S10 flips no longer wait for their deferred optional slices.
- **Ownership:**
  - Append-only shared docs and compose switch lines go in `lead_shared_files`, not in `files`. Implementers hand over the exact lines and the lead applies them after the batch. These are ledger.md, capability-ownership-matrix.md, ZDR.md, MODEL_PLANE_STATUS.md, master-ownership-matrix.md, both docker-compose.yml files and .env.example.
  - Everything else, including zdr-posture-matrix.json and sse-event-taxonomy.md, stays strictly disjoint per batch, because tests read those files.
- **Model per slice:** opus 5.5 for security, authority, audit, ZDR, money, data-loss and hard-design slices (81 records), haiku 5.5 max for mechanical ones (29) and sonnet 5.5 high for the rest (180).

## Needs the owner (exact questions)

1. **Provider routes:** Can you add or confirm these on the dev stack?
   - An OpenAI or Azure gpt-5-mini deployment.
   - Foundry access for claude-opus-4-8.
   - A vision-capable Azure or OpenAI deployment.
   - Staging OpenAI and Azure OpenAI routes, including whether Azure accepts `prompt_cache_key`.

   Without them, WS11-S1 to S5, WS08-S3/S4 and WS02-S8/S10 ship code-only, proven by tests.
2. **ZDR:** Is the Z-40 ZDR contract with Azure OpenAI signed? Can the operator provide the ZDR-TEST org credential, a ZDR-enabled test plan and the signed runtime public policy file? This affects WS01-S7, WS01-S13, WS10-S7, WS12-S7 and WS27-S5.
3. **Key ceremony:** When will the operator generate `MODEL_PLANE_ARTIFACT_SIGNING_KEY` and its verify key, and the `EXECUTION_CORE_PROVIDER_WRITE_ATTESTATION` keys? Until then the code is proven with dev-only ephemeral keys (WS01-S12, WS23-S8).
4. **Sign-ins:** Will you create the dedicated Microsoft 365 developer tenant and the dedicated Gmail test account, sign in, give OAuth consent and complete G12 publisher verification? This affects WS23-S1 and WS23-S3 to S8. Will you also connect a ChatGPT-subscription Codex account (WS11-S4, WS11-S5, WS08-S2)?
5. **Counsel:** Will counsel edit the public trust-claim wording in trust-claims.ts rows 36-41 (WS01-S14)? Commercial should also confirm that SIEM export is enterprise-gated. That confirmation is non-blocking, because the gate is a config flag.
6. **Infrastructure:** Can you provide a NATS credential for the mp.v1.security capture (WS26-S1) and a public HTTPS host for the indirect-injection fixture page (WS26-S2)?
7. **Branch:** WS21-S4 to S7 need the Quarry-side grant issuance endpoint and the Quarry-to-broker identity. Those exist only on the local branch `quarryv2`. Should `quarryv2` be merged into `model-plane` before those slices' live proofs? They are built against the contract with a stub until then.
8. **Repo setting:** Will you change the GitHub default branch from dev (DEC-39 D1(c), WS12-S1.ext)? This is non-blocking, because CI already runs on the integration branch.
