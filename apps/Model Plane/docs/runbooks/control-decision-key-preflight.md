# Control decision key preflight (F11)

Owner: Model Plane operator. Script: `apps/Model Plane/scripts/check-control-decision-key.sh`. Test: `apps/Model Plane/scripts/tests/check-control-decision-key-test.sh`.

## What it protects against

Control signs every Space decision (schedule fire, scheduled run, scheduled step, model action view) with a key it generates once and records in its ledger, `apps/Control Plane/.env.generated-secrets`. Model Plane services verify those decisions with `CONTROL_SPACE_DECISION_KEY_ID` and `CONTROL_SPACE_DECISION_PUBLIC_KEY_BASE64`. `scripts/compose.sh` loads the ledger first and `deploy/.env` second, and the later file wins. A stale copy of either variable left in `deploy/.env` therefore overrides the ledger, and from then on every Control-signed decision names a key id the service does not trust. In the W1 live proof this showed up only as a bare "untrusted ... key" refusal.

## Run it before starting or recreating the stack

```bash
cd "apps/Model Plane"
scripts/check-control-decision-key.sh
```

The script resolves the effective values the way `compose.sh` does (ledger, then `deploy/.env`, then the calling process environment, which Compose lets override both) and compares them with the ledger. It prints key ids, key lengths, the file each value came from and a short fingerprint (the first 8 bytes of `sha256` of the raw public key, as hex). It never prints a key value.

| Exit | Meaning |
|---|---|
| 0 | The effective id and public key match the ledger. |
| 1 | Mismatch, an unset pair, or a key that is not a base64/base64url encoding of 32 bytes. The message names the file that wins. |
| 2 | The check could not run (ledger missing or incomplete, bad arguments). |

To use it as a gate, opt in explicitly. It is not wired into `compose.sh`:

```bash
scripts/check-control-decision-key.sh && scripts/compose.sh up -d
```

For an artifact deployment, or any chain other than the default, name the files in `compose.sh` order (later files win):

```bash
scripts/check-control-decision-key.sh --ledger "<ledger file>" --model-env "<runtime env file>"
```

If `CONTROL_SPACE_DECISION_PUBLIC_KEYS_JSON` is set in the process environment the script says so and does not compare it; session-core reads that key set instead.

## Fixing a mismatch

Remove the stale `CONTROL_SPACE_DECISION_*` lines from the overriding file, so the ledger value is used. Do not copy the ledger values into `deploy/.env`; that is the drift this check exists to catch. Then recreate `capability-core`, `orchestrator-core`, `execution-core` and `session-core` so they read the new environment. Never edit or regenerate the Control ledger to match a stale Model Plane file.

## Start-up behaviour of capability-core and orchestrator-core

Both services now check the key when they start:

- With neither variable set, they log a warning and start; the Space decision lanes stay unavailable, as before.
- With a value set, both must be present and the public key must decode to 32 bytes. Otherwise the service logs the reason and exits, instead of starting with a lane that is quietly disabled. The decoder reads unpadded base64url first (the form Control emits) and accepts padded base64url and standard base64 as fallbacks, with a warning.
- When the key parses, the log line carries `decision_key_id` and `decision_key_fingerprint`. Compare the fingerprint with the script's output or with session-core's start-up line.
- When a decision names a key id other than the configured one, the refusal is unchanged, and an error is logged with `token_key_id` and `configured_key_id`, at most once a minute per token key id.
