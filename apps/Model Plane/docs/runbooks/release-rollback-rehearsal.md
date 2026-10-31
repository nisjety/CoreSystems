# Signed baseline, signed candidate and rollback rehearsal (WS01 S12)

Context: Model Plane promotes only through a signed artifact (`scripts/release-artifact.sh`, format v3) and only when a signed rollback artifact exists and a rollback rehearsal has passed. This runbook is the operator sequence that produces the pair, rehearses the rollback (`scripts/rollback-rehearsal.sh`, S10), deploys candidate, baseline, candidate, and records the evidence the conformance gate (`scripts/coresystem-conformance.sh`, S11) reads. The scripts are built and unit-tested; this sequence has not been run against real images. Until it has, no document may say a candidate or rollback exists.

Read `migration-policy.md` first: it explains why the rollback boots over a newer schema and why `SESSION_CORE_MIGRATION_IGNORE_MISSING` belongs to the rehearsal overlay alone.

## Two key regimes. Do not mix them up

| | Production | Dev-keyed proof |
|---|---|---|
| Keys | Created in the operator's key ceremony, held in the managed secret store | Ephemeral RSA pair made for one proof run, deleted afterwards |
| Result | May be cited as a signed candidate | Proves the pipeline works. It is never a candidate, never promotable |
| Where it is stated | Evidence file, release evidence register | The evidence file's first line says `DEV-KEYED`; the register says "pipeline proven with dev-only keys" and the Candidate and Rollback columns stay `absent` |

The agent that wrote this runbook did not and must not create, read or print a production key. Production key custody is an operator decision (plan decision Q-7a2-02). Everything below works with either pair; only the labelling differs.

Ephemeral keys for a dev-keyed run (the paths are absolute, outside the repository, mode 600 for the private key):

```bash
KEYDIR="$(mktemp -d)"; chmod 700 "$KEYDIR"
openssl genpkey -algorithm RSA -pkeyopt rsa_keygen_bits:2048 -out "$KEYDIR/devkey.pem"
openssl pkey -in "$KEYDIR/devkey.pem" -pubout -out "$KEYDIR/devkey.pub.pem"
chmod 600 "$KEYDIR/devkey.pem"
# ... run the sequence ... then: rm -rf "$KEYDIR"
```

Never paste a key, a runtime env value or a gates secret into a command line that is logged, a file in the repository, or an evidence file. Pass paths.

## Preconditions

- Linux or macOS host. `release-artifact.sh` opens its inputs with `O_NOFOLLOW`, which Windows Python does not support; every `release-*-test.sh` and every build fails there with "this platform does not support O_NOFOLLOW". Run the sequence, and those tests, on Linux (CI runs them in `model-plane-ops-gates.yml`, job `release-gates`).
- Docker on this host. A rehearsal start reuses the fixed compose project `model-plane` and network `model-plane-network`: stop the dev stack or use a second host. `rollback-rehearsal.sh` refuses when a `model-plane` project is running.
- `MODEL_PLANE_PRODUCTION` is not `1`. Rehearsal, `rebuild-service.sh` and this runbook refuse it.
- S0, S2, S7, S8, S9, S10 and S11 are committed, and the tree you build from is clean: both builds run from a clean detached worktree (`git worktree add --detach <dir> <revision>`), never from the working checkout. `release-artifact.sh build` refuses a dirty `apps/Model Plane`.
- `nats-provisioner` is in `SERVICES` and in the release overlay (S8). `release-artifact.sh dry-run <dir>` lists its build; if it does not, stop.
- The six checks of `zdr-attestation-and-live-proof.md` pass 1 are recorded: the ZDR evidence digest goes into the gates file below.

## 1. Baseline R (the rollback), non-release mode

The baseline is the last revision whose image set passes S8 to S11 (decision D12). Build it from a clean detached worktree at that revision, in non-release mode, signed with the same ceremony key:

```bash
git worktree add --detach "$BASELINE_WT" <D12 revision>
cd "$BASELINE_WT"
MODEL_PLANE_ARTIFACT_SIGNING_KEY="$KEYDIR/devkey.pem" \
  bash "apps/Model Plane/scripts/release-artifact.sh" build "$ARTIFACTS/baseline-<short revision>"
```

A non-release build needs no gates file and no rollback pointer; it records `not-attested` gates. A baseline from before S8 has no `nats-provisioner` image: the candidate's `validate-deployability` and the rehearsal's `compose <rollback> config` then say so, and the rehearsal evidence must name it. Prefer a baseline at or after S8.

Record the baseline's root manifest digest: `sha256sum "$ARTIFACTS/baseline-<short revision>/artifact-manifest.tsv"`. This is `ROLLBACK_ARTIFACT_MANIFEST_SHA256`.

## 2. The compatibility gates file

Release mode needs a non-secret file of exactly eleven assignments. Review each line with the owner; none is a default.

```
COMPATIBILITY_GATES_VERSION=1
SOURCE_REVISION=<candidate revision, 40 hex>
STATUS=passed
PROTOCOL_COMPATIBILITY=passed
MIGRATION_COMPATIBILITY=passed
LIVE_AUTHORIZATION=passed
APPROVAL_CONTINUATION=passed
ZDR_RETENTION_PATH=authoritative-non-zdr-policy-attested
ZDR_EVIDENCE_SHA256=<sha256 of docs/compliance/evidence/live-reads-zdr-<date>.txt>
AUTH_IDENTITY_EVIDENCE_SHA256=<sha256 of the auth identity evidence>
ROLLBACK_ARTIFACT_MANIFEST_SHA256=<section 1 digest>
```

- `ZDR_RETENTION_PATH` is `authoritative-non-zdr-policy-attested` until a provider deployment is attested (task Z-40). Use `zdr-provider-route-attested` only with the attestation recorded in `docs/ZDR.md` and `zdr-attestation-check.sh` passing on the runtime public policy.
- A gate set to `passed` without the evidence behind it is a compliance failure, not a shortcut. A dev-keyed run may use `passed` only for the pipeline; label the run.
- Every digest is of a committed or archived file, never of a placeholder. All zeros is refused.

## 3. Candidate C, release mode

```bash
git worktree add --detach "$CANDIDATE_WT" <candidate revision>
cd "$CANDIDATE_WT"
MODEL_PLANE_RELEASE_MODE=1 \
MODEL_PLANE_ARTIFACT_SIGNING_KEY="$KEYDIR/devkey.pem" \
MODEL_PLANE_ARTIFACT_VERIFY_KEY="$KEYDIR/devkey.pub.pem" \
MODEL_PLANE_ROLLBACK_ARTIFACT_DIR="$ARTIFACTS/baseline-<short revision>" \
MODEL_PLANE_ROLLBACK_RUNTIME_ENV_FILE="<baseline runtime env, path only>" \
MODEL_PLANE_ROLLBACK_RUNTIME_EVIDENCE_FILE="<baseline runtime evidence, path only>" \
MODEL_PLANE_COMPATIBILITY_GATES_FILE="<gates file of section 2>" \
MODEL_PLANE_RUNTIME_PUBLIC_POLICY_FILE="<runtime public policy, non-secret>" \
  bash "apps/Model Plane/scripts/release-artifact.sh" build "$ARTIFACTS/candidate-<short revision>"
```

The build checks every input before the first Docker command. If it stops there, the message names the missing input; fix that input, do not weaken the gate.

Then, without rebuilding:

```bash
export MODEL_PLANE_RELEASE_MODE=1 MODEL_PLANE_ARTIFACT_VERIFY_KEY="$KEYDIR/devkey.pub.pem" \
       MODEL_PLANE_ROLLBACK_ARTIFACT_DIR="$ARTIFACTS/baseline-<short revision>" \
       MODEL_PLANE_RUNTIME_ENV_FILE="<candidate runtime env, path only>" \
       MODEL_PLANE_ROLLBACK_RUNTIME_ENV_FILE="<baseline runtime env, path only>"
bash "apps/Model Plane/scripts/release-artifact.sh" verify   "$ARTIFACTS/candidate-<short revision>"
bash "apps/Model Plane/scripts/release-artifact.sh" restore  "$ARTIFACTS/candidate-<short revision>"
bash "apps/Model Plane/scripts/release-artifact.sh" compose  "$ARTIFACTS/candidate-<short revision>" config >/dev/null
bash "apps/Model Plane/scripts/release-artifact.sh" validate-deployability "$ARTIFACTS/candidate-<short revision>"
```

Discard the `compose ... config` output: it expands runtime values.

## 4. The rehearsal (S10)

Dry run first: it reads the two artifact trees, prints what it will do and writes nothing.

```bash
bash "apps/Model Plane/scripts/rollback-rehearsal.sh" --dry-run \
  "$ARTIFACTS/candidate-<short revision>" "$ARTIFACTS/baseline-<short revision>"
```

Read the plan it prints: how many session-core migrations the baseline does not know, and which are contract migrations with a revert file. A contract migration without a revert file stops the script, correctly.

Then the real run, on the second host or with the dev stack stopped:

```bash
MODEL_PLANE_ARTIFACT_VERIFY_KEY="$KEYDIR/devkey.pub.pem" \
MODEL_PLANE_RUNTIME_ENV_FILE="<candidate runtime env, path only>" \
MODEL_PLANE_ROLLBACK_RUNTIME_ENV_FILE="<baseline runtime env, path only>" \
  bash "apps/Model Plane/scripts/rollback-rehearsal.sh" --no-build [--start-stack] \
  [--seed-sql <synthetic seed>] "$ARTIFACTS/candidate-<short revision>" "$ARTIFACTS/baseline-<short revision>"
```

It writes `docs/compliance/evidence/rollback-rehearsal-<date>.txt`. Its first line is `# Rollback rehearsal: result PASSED` or `FAILED`; the conformance gate reads exactly that line and the date in the file name. The file holds names, versions, counts and pass/fail only.

What a pass means: the baseline `session-core` refused the candidate's migrations in strict mode (the drift detector works), then booted over them with the switch set in the rehearsal overlay (additive pair) or after the scripted reverts (contract pair), `compose <baseline> config` rendered, and the candidate passed `validate-deployability`. It does not mean the baseline served traffic; `--start-stack` starts it without building.

A `FAILED` file is evidence too. Keep it, fix the cause, rerun with another `--date` or `--evidence-file`: the script refuses to overwrite.

Dev-keyed run: after the script finishes, add `# DEV-KEYED: ephemeral keys, pipeline proof only, not a candidate` as the second line and say the same in the register row. Do not edit the first line.

## 5. Deploy candidate, baseline, candidate

The order proves both directions work on the same stack:

1. `release-artifact.sh deploy` the candidate. Run `scripts/tests/zdr-live-proof.sh --phase preflight` and a smoke turn (tool turn, thread read, approval).
2. Deploy the baseline over it (`deploy` of the baseline directory with the baseline runtime env). If the pair needs a scripted revert, do it exactly as the rehearsal did, before the baseline starts.
3. Deploy the candidate again.

Between steps wait for health; a deploy does not build (`up --no-build`). Stop at the first failure and keep the logs; do not carry on to make the sequence look complete.

## 6. Preflight and conformance

```bash
bash scripts/coresystem-cross-plane-preflight.sh
MODEL_PLANE_CANDIDATE_ARTIFACT_DIR="$ARTIFACTS/candidate-<short revision>" \
MODEL_PLANE_ROLLBACK_ARTIFACT_DIR="$ARTIFACTS/baseline-<short revision>" \
MODEL_PLANE_ARTIFACT_VERIFY_KEY="<public key path>" \
  bash scripts/coresystem-conformance.sh --profile runtime
CORESYSTEM_CONFORMANCE_PROFILE=runtime <the same three variables> \
  bash scripts/coresystem-conformance-artifact.sh "docs/compliance/evidence/conformance-runtime-<date>.json"
```

Both need `rg` (ripgrep) on `PATH`: the cross-plane preflight and the capability ledger test call it, and without it a missing evidence marker is reported as recorded. The preflight reads the register rows of `MODEL_PLANE_RELEASE_EVIDENCE.md` for open evidence; leave those rows saying `absent` until real evidence exists.

`--profile runtime` reads `ready` only on a clean tree, with both artifacts verified against the trusted key, a PASSED rehearsal no older than the threshold, a valid capability ledger and a valid thresholds file. `ready` for `runtime` is not promotion: `--profile full` additionally needs approved thresholds, a fully promoted capability ledger and clean dev-runtime and cross-plane preflights. It must still read `blocked` on a dev-keyed run; if it reads `ready` the key labelling above was skipped.

A blocker may be waived only by an unexpired row in `RELEASE_GATE_WAIVERS.tsv` (owner, reason, evidence, at most 30 days). Blockers that say the evidence is wrong (`*_unverified`, `dirty_worktree`, `artifact_verify_key_missing`) cannot be waived.

## 7. Record

1. Evidence files committed: `rollback-rehearsal-<date>.txt`, the conformance JSON, the manifest digests of both artifacts (`sha256sum artifact-manifest.tsv`), the revisions.
2. `MODEL_PLANE_RELEASE_EVIDENCE.md`: update the release-artifact row of the register and the promotion list: Candidate and Rollback columns move off `absent` only for a production-keyed run, with the digests; a dev-keyed run is recorded in the pipeline section and leaves them `absent`.
3. `MODEL_PLANE_STATUS.md`: the release-gate rows.
4. If the conformance report still names blockers, list them as they are. Do not edit the evidence marker rows of the register to make the preflight pass.

## Rollback of this procedure

Nothing here changes a running stack until section 5. Rolling back section 5 is a deploy of the baseline (section 5, step 2), with the contract-migration revert from `migration-policy.md` first when the pair needs one. Remove the ephemeral keys and the detached worktrees (`git worktree remove`) when a dev-keyed run is over.
