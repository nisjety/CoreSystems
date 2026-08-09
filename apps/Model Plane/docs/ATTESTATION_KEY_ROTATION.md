# Provider-write attestation key rotation

**Status: provisioned, not yet exercised in prod.** Everything below works
today (verified against the real `ParseTrustedKeysJSON` and a full
stage → promote → prune cycle) — this is not a plan for future code, it is
the runbook for a rotation that has not yet needed to happen. It applies to
the `model-execution` issuer's key (execution-core's signer,
`src/attestation.rs`); `conversation-core`'s key uses the same trust registry
and can be rotated the same way once an equivalent script command exists for
it — only `model-execution` has one today.

## Why rotation is low-risk here

Two properties of this design make rotation additive rather than a cutover
with a blast radius:

- The trusted-key registry
  (`INTEGRATION_PROVIDER_WRITE_ATTESTATION_KEYS_JSON`, read by
  integration-corev2's `ParseTrustedKeysJSON`) is a **list keyed by `kid`**,
  not a single key per issuer. Two entries for the same issuer with
  different kids are simultaneously valid — this is exactly what a rotation
  window needs and required no schema change to support.
- Every attestation JWS has a **30-second TTL**
  (`DEFAULT_TTL_SECONDS` in `attestation.rs`). There is no long-lived
  credential in flight the way there would be for, say, an API key with a
  90-day rotation window — once the signer stops using the old key, every
  token it ever signed with that key is unusable within 30 seconds
  regardless of whether the old key is still in the trusted registry. The
  "bake period" before it's safe to remove the old key is minutes, for
  auditability, not the days typical of long-lived credential rotation.

## The three commands

Run from the repo root. Each is a standalone action — it touches only the
one thing it's asked to and exits; it is not a step inside the full
`bootstrap_runtime_environment.sh` provisioning sequence and does not
require (or risk) re-running anything else that script does.

```bash
# 1. Stage a new key (kid incremented, e.g. v1 -> v2) as NEXT. The active
#    key is untouched — the signer keeps using it. The new key's public half
#    is added to the trusted registry immediately, so it can be validated
#    (e.g. against a canary execution-core instance configured to sign with
#    the NEXT private key) before anything depends on it in production.
./scripts/bootstrap_runtime_environment.sh --rotate-model-execution-attestation-key=stage

# 2. Promote: the signer's active key becomes NEXT (execution-core must be
#    restarted/redeployed after this so it picks up the new
#    EXECUTION_CORE_PROVIDER_WRITE_ATTESTATION_PRIVATE_KEY). The old active
#    key moves to PREVIOUS and stays in the trusted registry.
./scripts/bootstrap_runtime_environment.sh --rotate-model-execution-attestation-key=promote

# 3. After the bake period (minutes — see above, not days), prune the old
#    key from the trusted registry.
./scripts/bootstrap_runtime_environment.sh --rotate-model-execution-attestation-key=prune
```

Add `--dry-run` to any of the three to preview without writing (each command
logs exactly what it would do, including refusing cleanly when there is
nothing to act on — e.g. `promote` before `stage` has ever run).

## What each command actually touches

| Step | File | Variable(s) |
|---|---|---|
| stage | Model Plane `.env` | `EXECUTION_CORE_PROVIDER_WRITE_ATTESTATION_PRIVATE_KEY_NEXT` (new, inactive private key) |
| stage | integration-corev2 `.env` | `EXECUTION_CORE_PROVIDER_WRITE_ATTESTATION_PUBLIC_KEY_NEXT` / `_KEY_ID_NEXT` |
| promote | Model Plane `.env` | `EXECUTION_CORE_PROVIDER_WRITE_ATTESTATION_PRIVATE_KEY` becomes the staged key; `_NEXT` cleared |
| promote | integration-corev2 `.env` | active `PUBLIC_KEY`/`KEY_ID` become the staged ones; the prior active pair moves to `_PREVIOUS`; `_NEXT` cleared |
| prune | integration-corev2 `.env` | `_PREVIOUS` cleared |
| every step | integration-corev2 `.env` | `INTEGRATION_PROVIDER_WRITE_ATTESTATION_KEYS_JSON` reassembled from whichever of active/NEXT/PREVIOUS are currently populated |

`promote` is the only step that requires a redeploy: it changes the private
key execution-core reads at startup. `stage` and `prune` only change what
integration-corev2 trusts — no execution-core restart needed for either.

## Operational sequence for a real rotation

1. Run `stage`.
2. Redeploy/restart integration-corev2 so it picks up the widened trusted
   registry (or, if integration-corev2 already re-reads its env on a config
   reload path, trigger that instead of a full restart).
3. Optionally validate the NEXT key end to end against a canary
   execution-core instance configured to sign with it, before touching the
   fleet's primary signer.
4. Run `promote`.
5. Redeploy/restart execution-core so it signs with the newly active key.
6. Wait the bake period (a few minutes is generous given the 30s TTL).
7. Run `prune`.
8. Redeploy/restart integration-corev2 once more so the pruned registry
   takes effect (the old key stops being trusted).

## What this does not yet cover

- **conversation-core's key** uses the same registry and the same
  `kid`-keyed design supports rotating it identically, but no
  `--rotate-conversation-core-attestation-key=...` command exists yet — only
  `model-execution`'s. Add the equivalent three functions
  (`rotate_conversation_core_attestation_key` etc.) if/when it needs
  rotating; the pattern in `scripts/bootstrap_runtime_environment.sh` is
  directly copyable.
- **Automatic/scheduled rotation.** This is a manually-triggered runbook, not
  a cron job. Nothing here decides *when* to rotate — that remains an
  operator/security decision (a suspected compromise, a periodic policy, a
  personnel change with prior key access, etc.).
- **Cross-plane credential propagation for the rotation commands
  themselves.** Like every other cross-plane secret in this system today
  (see `verevon-roadmap.md` on the credential-rollout ritual), these commands
  write to each plane's own local `.env` — getting the resulting values into
  a real multi-host prod deployment (rather than this single-host local
  Docker setup) is the same not-yet-automated step every other secret in
  this system needs, not a gap specific to attestation keys.
