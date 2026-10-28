# Content safety for external tool results

Status: the inline injection pass (WS26-S6) is built behind default-off switches. It is source-verified only. No threshold has been evaluated (WS26-S8), so no default has changed and every org stays in `frame` mode.

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

## Rollout

- The default is `frame`. Nothing changes for any org until these switches are set.
- Turning `tighten` on for an org requires an archived evaluation (WS26-S8: deterministic benign false positives zero, semantic benign false positives at most 1%, paraphrase recall at least 85%, p95 added latency at most 2.5 s) and a week of counters.
- `enforce` is set per org, after the same evaluation, and never for third-party MCP results before WS05.
- To roll back, set the org row to `frame`, or set `CONTENT_SAFETY_SEMANTIC_INLINE` to `false`. With the inline switch off, the code path is the one that ran before this slice.

## Known limits

- The call budget belongs to one tool loop. A turn that runs a forced search and then the tool rounds can spend up to twice the cap. Moving the budget up to the turn owner is a follow-up.
- Gateway-orchestrated deep research web calls carry no inference bearer, so they get no inline pass. They keep the deterministic scan.
- The `SCREENING` note in the model's context still says "a prompt-injection marker" for a semantic flag too.
- No evaluation has run, so the defaults stay conservative.
