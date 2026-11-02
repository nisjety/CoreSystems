# Model Plane asks from the Frontend and Quarry lanes (lane d, w23)

One section per unit. Each ask names the file, the exact change and why.

## W2-FE-L5a, WS15.L5 (admission contract re-vendor)

The Verevon v3 action registry changed, so the agent admission golden the SPA generates changed. The SPA copy is already regenerated and committed (`apps/Frontend Plane/verevonv3/src/shared/api/contracts/agent-action-admission.json`, LF sha256 `bcf3d6ffbf020946ceb76994bd9e6ef0cccb7e249d1ed2fa5c95318bd51c0482`). The repo-level drift check `scripts/tests/zdr-contract-sync.test.sh` fails until the Model Plane canonical copy and the three vendored copies match it.

- **Model Plane (contracts owner):** copy the SPA file byte for byte over `apps/Model Plane/api/contracts/agent-action-admission.json`; refresh the pin `apps/Model Plane/api/contracts/agent-action-admission.json.sha256` (`sha256sum`, keep the `<hash>  <name>` form).
- **Repo script:** in `scripts/tests/zdr-contract-sync.test.sh` `GOLDENS` (line 204) replace the digest `13f3b72a93c68868b6a4108e1584aec2adf4d0bf40becee878d6bf548d38eb3e` with `bcf3d6ffbf020946ceb76994bd9e6ef0cccb7e249d1ed2fa5c95318bd51c0482`.
- **Vendored copies (copy file and pin):** `apps/Model Plane/go/services/capability-core/internal/api/testdata/agent-action-admission.json`, `apps/Model Plane/rust/services/execution-core/tests/fixtures/agent-action-admission.json`, `apps/Model Plane/rust/services/model-gateway/tests/fixtures/agent-action-admission.json`. Run the three consumers' tests (the model-gateway test checks every Ask-loop read arm against the rows).
- **What changed in the rows:** `ingestions.run_schedule_action` is gone; added `ingestions.get_run_status` (side effect `read`), `pause_run`, `resume_run`, `cancel_run`, `pause_schedule`, `change_schedule_state` (all `access: none`, `exemptReason: no_owner_executor`, `external_write`); `ingestions.create_schedule` has a new schema hash. No row is eligible, so no consumer's executable set changes.
