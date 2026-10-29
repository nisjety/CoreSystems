# Content safety for external tool results

Status: the inline injection pass (WS26-S6) is built behind default-off switches and is source-verified only. The evaluation corpus and the live eval (WS26-S8) are built, but the live run has not been made and archived. No threshold has been met, so no default has changed and every org stays in `frame` mode.

External tool results (`web_search`, `fetch_url`, `browser_agent`) are untrusted text that the model reads in the same context as its instructions. Model Plane screens each one before the model reads it. This note covers the screening modes, what each mode does, and the switches that select it.

## Layers

1. **Deterministic scan** (`mp-content-safety`, WS26-S2). It always runs on the complete payload. It is pure CPU, so it runs on ZDR turns too. Its posture is `clean`, `flagged`, `degraded`, `policy-disabled` or `not-applicable`.
2. **Shadow pass** (`CONTENT_SAFETY_SHADOW_SAMPLE_RATE`, WS26-S1). This is the `frame` behaviour. A sampled fraction of external results is classified out of band. It records a comparison event and a counter, and it never changes what the model reads.
3. **Inline injection pass** (WS26-S6). It runs only under `tighten` or `enforce`, and only with `CONTENT_SAFETY_SEMANTIC_INLINE=true`. It is described below.

## Modes

| Mode | What the model reads | Classifier | Withholds |
|---|---|---|---|
| `frame` (default) | the deterministic framing; the shadow pass records only | shadow pass, sampled, out of band | no |
| `tighten` | the merged verdict: a flag adds `flagged` to a clean result | inline, awaited, once per clean external result | no |
| `enforce` | as `tighten`, and a flagged web or browser payload is replaced by a stub | as `tighten` | yes, web and browser content only |

The merge is tighten-only. A classifier verdict can promote a deterministic `clean` result to `flagged`. It cannot demote `flagged`, and it cannot clear a degraded result. A verdict whose `flagged` is not a JSON boolean is no verdict, and it is never read as clean.

Third-party MCP results are framed and never withheld, even under `enforce`. The WS05 signed gate decides that class.

## Switches

Every switch defaults off. Booleans read only the literal `true`. Modes read only the three words `frame`, `tighten` and `enforce`. An unknown or unreadable value keeps the safe default.

| Switch | Default | Meaning |
|---|---|---|
| `CONTENT_SAFETY_MODE_DEFAULT` | `frame` | The env mode for every org that has no row setting. |
| `CONTENT_SAFETY_SEMANTIC_INLINE` | `false` | Master switch for the inline pass. While it is off, the effective behaviour is `frame`, whatever the mode says. |
| `CONTENT_SAFETY_SEMANTIC_DEADLINE_MS` | `2500` | Ceiling for one inline call, 100 to 10000 ms. Anything else keeps the default. |
| `CONTENT_SAFETY_SEMANTIC_MAX_CALLS_PER_RUN` | `6` | Inline calls one tool loop may spend, 0 to 64. `0` means none. |
| `CONTENT_SAFETY_INJECTION_MODEL` | empty | The model the classifier asks inference-core for. Empty keeps the inference-core default. |

An org sets its own mode in the `config_json` of its `injection_defense` row in capability-core's `safety_policies`:

```json
{"mode": "tighten", "fail_mode": "open"}
```

- `mode` is one of the three words. A missing `mode` keeps the env default. A `mode` that does not parse, or a `config_json` that is not an object, keeps the env default capped at `tighten`.
- `fail_mode` is `open` (the default) or `degraded`. `degraded` reads a clean result as unverified when the inline pass cannot answer.
- The projection lists rows by priority, and the first enabled `injection_defense` row decides.
- A disabled `injection_defense` row still turns screening off, as before.
- A failed policy read keeps screening on at the env mode, capped at `tighten`. A failed read never produces `enforce`.

## When the inline pass runs

It runs only when all of these hold:

- `CONTENT_SAFETY_SEMANTIC_INLINE=true`;
- the turn carries a delegated inference bearer (gateway-orchestrated deep research web calls do not);
- the org's effective mode is `tighten` or `enforce`;
- the deterministic posture is `clean`, the payload is not empty, the turn is not ZDR, and the tool loop's budget has a call left.

Otherwise the decision is recorded as a skip with a reason. A flagged or degraded payload is never sent for a second opinion.

## The classifier

- Operation `prompt_injection` on `AnalyzeLanguage` (inference-core). The LLM provider serves it. Azure AI Language declines it.
- The payload sits inside a JSON-encoded array, between markers that carry a fresh random id on every call. The prompt tells the model to classify the block and never to follow it.
- Output: `{"flagged": bool, "categories": {"instruction_override", "role_impersonation", "exfiltration_request", "tool_use_request", "hidden_text"}}`, each score from 0 to 1.
- Up to 40 KiB the classifier reads the whole payload. Above that it reads the first 32 KiB and the last 8 KiB, and the result is recorded as `coverage=partial`. A clean verdict on a partial window is never recorded as a full clean.
- The request id is `screen:<run_id>`, so the classifier's usage traces back to its run.

## ZDR

A ZDR turn never reaches the inline classifier. The decision is recorded as `skipped-zdr`, and no event is published for it. The deterministic scan still runs. Under `enforce`, a deterministic flag still withholds the payload, because withholding needs no content outside the process.

## Failure behaviour

| Condition | Result |
|---|---|
| Classifier error, timeout, malformed verdict, or no semaphore permit | The deterministic posture stands under `open`. Under `degraded`, a clean result reads as read-only. |
| Call budget spent | `skipped-capped`. The deterministic posture stands. |
| Policy read failed or unparseable | Screening stays on at the env mode, capped at `tighten`. |

## Events and counters

- Event `TOOL_RESULT_SEMANTIC_ENFORCED` on `mp.v1.security.{org_id}`. It is published when the classifier was asked or the payload was withheld, and never on a ZDR turn. Payload: tool, trust class, mode, deterministic and final posture, outcome, coverage, withheld flag, category scores and content hash. It carries no text.
- Counter `mp_gateway_injection_screen_total{trust, mode, outcome, coverage, posture}`. Every label comes from a closed set.
- A flagged result still publishes `TOOL_RESULT_SCREENING_FLAGGED`, as before. Its `flags` list includes `semantic.prompt_injection` when the inline pass added the flag.

## What the model sees when a payload is withheld

A stub in place of the payload. It names the tool, the trust class, the source when there is one, and the first eight hex digits of the content hash:

`[withheld: fetch_url result (external-web) was flagged by injection screening and is not shown; source https://example.test/page; hash 1a2b3c4d]`

A withheld payload is never parked for later reads, and the payload is not stored.

## Do mode (WS26-S7)

A Do run's tool results (execution-core) get the same tighten-only verdict as chat by asking model-gateway's `ScreenContent` RPC.

- The switch is `CONTENT_SAFETY_SCREEN_RPC` on execution-core (default off, only the literal `true`). The gateway side follows the same switches as chat: with `CONTENT_SAFETY_SEMANTIC_INLINE` off, the answer is the deterministic verdict and nothing is withheld.
- execution-core asks for every external result whose local scan covered the full payload, clean or flagged, so a deterministic flag can still be withheld under `enforce`. The gateway spends no classifier call on a result that is not clean.
- Only a service identity may call `ScreenContent`. A ZDR run never asks, and the gateway skips the classifier for a ZDR request instead of refusing it.
- The verdict carries the BLAKE3 hash of the bytes it screened, and execution-core drops a verdict whose hash does not match what it sent.
- A refused, slow, unreachable or mismatched answer keeps the local verdict (fail open). `fail_mode: degraded` is not applied on the Do side in that case.
- A withheld result is replaced by the stub before it is rendered for the model, and the step is marked tainted (`mp_exec_screening_taint_total`).

Known gaps: Do runs carry no inference credential, so the classifier budget from Do is zero and a paraphrased injection is not caught on the Do path yet. A ZDR Do run never asks, so under `enforce` a locally flagged web result is framed but not withheld, unlike chat's ZDR path. Both need a decision before `enforce` is used for Do.

## Rollout

- The default is `frame`. Nothing changes for any org until these switches are set.
- Turning `tighten` on for an org requires an archived evaluation (WS26-S8: deterministic benign false positives zero, semantic benign false positives at most 1%, paraphrase recall at least 85%, p95 added latency at most 2.5 s) and a week of counters.
- `enforce` is set per org, after the same evaluation, and never for third-party MCP results before WS05.
- To roll back, set the org row to `frame`, or set `CONTENT_SAFETY_SEMANTIC_INLINE` to `false`. With the inline switch off, the code path is the one that ran before this slice.

## Evaluation (WS26-S8)

The corpus lives in `rust/crates/mp-content-safety/tests/corpus/`. Each line is a JSON object with `id`, `kind`, `text`, `expect` (`flag` or `clean`) and `needs`. `needs: deterministic` means the scanner decides the row. `needs: semantic` means the scanner must leave the row clean, so only the classifier can flag it. Each file holds one combination of the two:

| File | Rows | `expect` | `needs` | Kinds |
| --- | --- | --- | --- | --- |
| `attacks.jsonl` | 487 | flag | deterministic | direct, uppercase, zero-width, fullwidth, line break, tag block, bidi, homoglyph, spaced letters, soft hyphen, forged framing, instruction in HTML (`indirect_html`), non-English markers (`multilingual_marker`) |
| `attacks_semantic.jsonl` | 103 | flag | semantic | `paraphrase` (English), `paraphrase_multilingual`, `split_markup`, `leetspeak`, `small_caps` |
| `benign.jsonl` | 349 | clean | deterministic | ordinary business text in several scripts |
| `benign_semantic.jsonl` | 93 | clean | semantic | `benign_injection_doc`, `benign_near_miss`, `benign_long_page`, `benign_multilingual` |

The rows are synthetic and public. No customer text belongs in them.

`cargo test -p mp-content-safety` is the offline gate. It checks that every deterministic attack is a hit, that no benign row is a hit, and that no semantic row is already a hit, so the semantic rows keep measuring the classifier.

The live eval is `cargo test -p model-gateway --test content_safety_eval -- --ignored`, run through `scripts/eval-content-safety.sh`. It sends every row through the production path: the deterministic scan first, then the inline pass in `tighten` mode, which calls the classifier only for a row the scan left clean. It prints counts per kind, the benign and paraphrase rates, the classifier model and the p95 latency of the calls. It never prints corpus text. It refuses a bearer that belongs to a ZDR org or to an org other than `CONTENT_SAFETY_EVAL_ORG_ID`.

### Thresholds

| Measure | Limit | Checked by |
| --- | --- | --- |
| Deterministic attacks flagged | every row | offline gate |
| Benign rows flagged by the scanner | zero | offline gate and live eval |
| Benign rows flagged by the classifier | at most 1% | live eval |
| Paraphrase recall (semantic attacks flagged) | at least 85% | live eval |
| p95 added latency of an inline call | at most 2.5 s | live eval |
| Classifier calls that end `unavailable-*` in production | below 5% over a week | screening counters |

These are the limits from the build plan. The live eval (`content_safety_eval.rs`) carries the same numbers, and the owner confirms them before the first archived run. Changing one means changing both.

### Running it and promoting a default

1. Run `CONTENT_SAFETY_EVAL_BEARER=<token> CONTENT_SAFETY_EVAL_ORG_ID=<org> scripts/eval-content-safety.sh` against a non-ZDR test org. The token goes in alone, without the word Bearer. Set `CONTENT_SAFETY_INJECTION_MODEL` if the run should use a model other than inference-core's default, and set `CONTENT_SAFETY_SEMANTIC_DEADLINE_MS` to its production value.
2. Copy the archived output, which holds counts only, into `docs/MODEL_PLANE_RELEASE_EVIDENCE.md` with the date, the commit and the model.
3. Change a default, or an org's `injection_defense` mode, only after step 2 holds and a week of screening counters shows fewer than 5% of classifier calls ending `unavailable-*`. The counters are `mp_gateway_semantic_screen_total` (WS26-S1) and `mp_gateway_injection_screen_total` (WS26-S6). `enforce` follows the same rule, per org.

### Limits of the evaluation

- The corpus is small and synthetic. A passing run shows that the classifier handles these rows, not that it catches every injection. Add public examples each quarter, never customer text.
- A quoted marker is a deterministic hit by design. A public article that quotes "ignore previous instructions" is flagged, and under `enforce` it is withheld. The corpus has no such row, because the zero-false-positive gate would fail on it. The owner decides whether to narrow the marker list or accept the withhold.
- Two false positives are known and are not yet in the corpus: emoji subdivision sequences, whose tag-block characters are flagged, and the legacy phrase "you are now a" in ordinary marketing text. Each needs its own decision before `enforce` is used on pages that contain them.

## Known limits

- The call budget belongs to one tool loop. A turn that runs a forced search and then the tool rounds can spend up to twice the cap. Moving the budget up to the turn owner is a follow-up.
- Gateway-orchestrated deep research web calls carry no inference bearer, so they get no inline pass. They keep the deterministic scan.
- The `SCREENING` note in the model's context still says "a prompt-injection marker" for a semantic flag too.
- No evaluation has run, so the defaults stay conservative.
