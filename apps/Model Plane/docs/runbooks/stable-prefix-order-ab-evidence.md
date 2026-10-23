# Stable-prefix prompt order: staging A/B and evidence (WS02-S8)

**Status: template.** Nothing below is measured yet. Fill the result and the decision from a real run. Do not carry numbers over from another run, and do not write them from the code.

## What the run decides

`MODEL_GATEWAY_CACHE_STABLE_PROMPT_ORDER` moves the per-turn blocks out of the cached prefix. It changes the prompt only on a turn pinned to an OpenAI-API chat-completions provider (`CacheRoute::of_turn` in `apps/Model Plane/rust/services/model-gateway/src/prompt_context.rs`). The A/B therefore pins `provider` to `openai` or `azure-openai` on every request. The question is whether the flag raises the prefix hit rate on that traffic without moving eval pass rate. The owner decides, and the decision goes in `apps/Model Plane/docs/decisions/ledger.md`.

## Before the run

- The routes the driver uses must be deployed: model-gateway `POST /v1/invoke/stream` and cost-core `GET /api/v1/cost/analytics` (published on `127.0.0.1:8089`). The `plan` command checks the route paths in the source only. It does not prove the deployment.
- The provider budget covers 26 turns per arm plus the eval suite three times per arm.
- `make eval` runs `$(VENVDIR)/bin/pytest`, which exists only in a POSIX venv. Run the eval steps from WSL or a POSIX shell. `make venv` in `apps/Model Plane/python/eval-lab-py` creates that venv and installs the package in editable mode, so `python -m eval_lab.stable_prefix_ab` resolves there.
- Set these in the shell, never in a file in this folder: `INTERNAL_API_KEY` (token minting for `drive` and `make eval`) and `EVAL_COST_TOKEN` (a cost-core token with `cost:org:read` for the eval organisation, for `capture`).
- Use absolute paths for `--evidence-root` and for `EVAL_REPORTS_DIR`. `make eval` runs from the eval-lab folder, so a relative path would land there.

Evidence root: `<repository root>/docs/eval-reports/ws02-s8-stable-prefix`. Commands below use `<root>` for it.

## Procedure

Run the driver from `apps/Model Plane/python/eval-lab-py` as `python -m eval_lab.stable_prefix_ab <command>`.

**Step 0, dry run (sends nothing).** Exit 0, 26 requests, both routes `route_found: true`.

    python -m eval_lab.stable_prefix_ab plan --provider <openai|azure-openai> --model <model>

**Step 1, arm off.**

1. Leave the switch unset in the environment compose reads, then recreate model-gateway: `bash "apps/Model Plane/scripts/compose.sh" up -d --no-deps model-gateway`. Confirm the value of `MODEL_GATEWAY_CACHE_STABLE_PROMPT_ORDER` in the container is empty. Inspect that one variable only; do not open the env files.
2. Drive the traffic: three threads of eight turns and two Do runs.

        python -m eval_lab.stable_prefix_ab drive --arm off --provider <p> --model <m> --evidence-root <root>

3. Capture the analytics for the arm's window. Run this after `drive` has finished, with `EVAL_COST_TOKEN` set.

        python -m eval_lab.stable_prefix_ab capture --arm off --evidence-root <root> --cost-core-url http://127.0.0.1:8089

4. Run the eval suite three times. Each rerun needs its own folder, because the report is named by date and reruns in one folder overwrite each other.

        EVAL_REPORTS_DIR=<root>/off/eval/rerun-1 make eval
        EVAL_REPORTS_DIR=<root>/off/eval/rerun-2 make eval
        EVAL_REPORTS_DIR=<root>/off/eval/rerun-3 make eval

**Step 2, arm on.**

1. Set `MODEL_GATEWAY_CACHE_STABLE_PROMPT_ORDER=1` in the environment compose reads. Recreate model-gateway with the same command and confirm the container shows `1`.
2. Drive and capture with `--arm on`. `drive` refuses an arm folder that already holds a drive, so a repeat needs a new evidence root.
3. Run the eval suite three times into `<root>/on/eval/rerun-N`.

Keep everything else identical between the arms: model, provider, thread script, rerun count. The companion switches `MODEL_GATEWAY_COMPACTION_TOKEN_BUDGET` and `MODEL_GATEWAY_DEDUP_CONTEXT_HISTORY` go in a separate run, one change at a time.

**Step 3, compare.**

    python -m eval_lab.stable_prefix_ab compare --evidence-root <root>

Paste the output under Result without editing the numbers.

## How to read the table

- **Hit rate** is cache read tokens over cache-reported input, the ledger's definition. "No cache activity reported" means the route reported none. It is not zero.
- **Write share** is cache-creation tokens over cache-reported input. OpenAI-API routes report cached prompt tokens and may report no cache-creation count, so expect 0.0% there. Check a turn record before reading it as "no writes".
- **Share of all input on pinned route**: below 100% means some traffic was not pinned. The table counts only the pinned share.
- **Off the pinned route** in the drive line counts turns whose usage frame names another provider, or none. Any value above 0 makes that arm invalid.
- **Eval pass rate** counts the cases that ran. A case skipped for a missing fixture does not count.
- **Eval parity** lists each case whose set of verdicts over the reruns differs between the arms. A flip that both arms share does not appear here; the min and max in **Eval pass rate** show the flips inside each arm.
- A failed turn or an invalid arm means a rerun under a new evidence root. Do not patch the arm.

## Result

Paste the table, the date, the commit (`git rev-parse --short HEAD`), the image tags of model-gateway and cost-core, and the `run_id` values from `<root>/off/turns.json` and `<root>/on/turns.json`.

## Owner decision

Record the decision in `apps/Model Plane/docs/decisions/ledger.md`, newest first, and tick one:

- [ ] Keep the switch off and measure again with: ____
- [ ] Turn it on for pinned OpenAI-API routes
- [ ] Turn it on only for: ____

Decided by: ____  Date: ____

## Rollback

Unset `MODEL_GATEWAY_CACHE_STABLE_PROMPT_ORDER`, or set it to anything other than `1`, `on`, `true`, `yes` or `enabled`. Recreate model-gateway and confirm the container value. The flag only shapes the prompt on pinned routes, and nothing is migrated.

## Privacy

`turns.json` holds counts, identifiers and error codes. `analytics.json` holds aggregate counts. Neither holds prompt or answer text, and neither holds a token. The eval reports are the harness's own output, unchanged. Keep the evidence root to these files.

## Code

- `apps/Model Plane/python/eval-lab-py/src/eval_lab/stable_prefix_ab.py`: `plan`, `drive`, `capture`, `compare`.
- `apps/Model Plane/python/eval-lab-py/tests/test_stable_prefix_ab.py`: offline unit tests, run with `tests/test_runner.py`.
