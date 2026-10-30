# H5: code_interpreter on Terra, runbook for the live proof (2026-10-01)

Task H5 of [the gap closure plan](CHAT_GAP_CLOSURE_PLAN_2026-10-01.md). Everything below the line "What is built" was done offline on 2026-10-01; **no Terra turn has been made and nothing has been rebuilt or deployed**. This note says what is proven, what is not, and exactly what the operator runs to prove the rest.

## What is built (offline)

| Piece | Where | State |
|---|---|---|
| The tool description now names every library the sandbox image ships (numpy, pandas, matplotlib, openpyxl, xlsxwriter, python-docx, reportlab, pypdf, Pillow, python-pptx, lxml) and says how each Office output is made: a native Excel chart with `openpyxl.chart` or xlsxwriter, formulas as `"=SUM(B2:B9)"` strings, a chart on a slide as a matplotlib PNG with `add_picture`, PDFs with reportlab, PNG charts with `plt.savefig`. | `model-gateway/src/tool_loop.rs`, `builtin_tool_defs` | Built, unit-tested. **Not in the running image.** |
| Drift guard: the description's "Available libraries" list must equal the python packages in `execution-core/Dockerfile` (read from the file). | `tool_loop.rs` tests `code_interpreter_description_names_every_library_the_sandbox_image_ships`, `..._says_how_to_make_each_office_output` | Both failed on the old description first, naming Pillow, lxml, pypdf, python-pptx and xlsxwriter. |
| The health attestation now imports the libraries (`numpy`, `pandas`, `matplotlib.pyplot`, `openpyxl.chart`, `xlsxwriter`, `docx`, `reportlab.pdfgen.canvas`, `pypdf`, `PIL.Image`, `pptx`, `lxml.etree`) instead of running `python3 --version`. A missing library keeps `cap.command.sandbox` unattested and is logged at warn by name. The attestation reason now reads "Bubblewrap sandbox probe succeeded and the interpreter imported every library code_interpreter advertises." | `execution-core/src/health_attest.rs`: `SANDBOX_PYTHON_MODULES`, `InterpreterProbe`, `interpreter_probe`, `probe_python_modules`, `probe_with_script` | Built, unit-tested. **Not in the running image.** A second drift guard compares the module list with the Dockerfile. |
| `GeneratedFiles` characterisation test (download anchor with data URI, mime label, size, open-in-tab only for https, PNG as preview not card). | `src/features/chat/components/ChatMessages.generated-files.test.tsx` | 6 tests, pass. |
| The live proof: five prompts, stream capture with timestamps, DOM and download checks, the oracle, judging, evidence. | `tests/e2e/product-code-interpreter.spec.ts`, `tests/e2e/code-interpreter-proof.ts`, `tests/code-interpreter-proof.test.ts`, `playwright.config.ts` (one regex entry) | Harness proven by a dry run on the real page with a mocked stream. Live cases written, **never run**. |
| The oracle: reads each file with the real library for its type, in the stack's own image. Self-tests that it accepts the reference files and rejects eight kinds of broken file. | `tests/e2e/fixtures/code-interpreter/h5_oracle.py` and `programs/make_*.py` | `self-test` passes. |

The plan named `product-office-outputs.spec.ts` for the live spec. That file already exists (H1's extraction proof, uncommitted, same working tree), so the H5 spec is a separate file, `product-code-interpreter.spec.ts`.

## What was checked against the live stack, read-only

- All eleven libraries import in the running `model-plane-execution-core-1` (python 3.11.2; openpyxl 3.0.9, python-docx 0.8.11, reportlab 3.6.12, pypdf 3.4.1, Pillow 9.4.0, python-pptx 1.0.2, matplotlib 3.6.3, pandas 1.5.3, numpy 1.24.2, xlsxwriter 3.0.2, lxml 4.9.2). The deep imports of the new probe take 1.2 to 1.7 s there.
- The five reference programs ran inside that container under the same `bwrap` argument list the sandbox builds (`--ro-bind / /`, `--clearenv`, `--unshare-net`, workspace bound read-write, `MPLBACKEND=Agg`), each in 0.13 to 1.1 s, against a 30 s limit. The oracle, run in the same container, accepted all six files they wrote (a native bar chart bound to cell ranges and 6 formula cells including 3 SUMs; a 4x3 table; a PDF whose extracted text holds æ, ø and å; a deck with a 600x360 picture; two PNGs that are not blank). Temp files were removed.
- `cap.command.sandbox` is `available` / `code_runtime_probed` (low risk, agentic, bounded), attested every minute, with the old `--version` probe.
- `session_tool_audit_intents` holds 148 completed and 3 failed `code_interpreter` rows (2026-09-14 to 2026-09-20). The table does not record the route, and the plan says all earlier live records are Verevon Balance, so this shows the gateway to execution-core to sandbox to files path has run for real; it is **not** evidence about Terra.
- The running `model-gateway` and `execution-core` binaries are the old build: the gateway binary holds the old description and not the new one, and the execution-core binary does not hold the new attestation reason. A rebuild is needed before the live proof (below).

Not verified, and why: no Terra tool decision (that is what the live run proves); the gateway's dispatch of a real call from a chat turn (needs a model); whether Terra picks `code_interpreter` over `create_artifact` for "Word-dokument" (the open question).

## Before the live run

1. **Wait for the Codex reset**, 2026-10-03 17:45 UTC. Each case is one chat turn, which is several Codex calls (a tool-decision round per tool call plus the answer stream): plan for 15 to 25 calls over the five cases. A cap hit mid-series is recorded and stops it; never switch model.
2. **Rebuild the model plane through the launcher** (never a hand-run `docker compose build`; a model-plane-only copy with `LAST_STACK_INDEX=2` avoids restarting the frontend). Services that changed: model-gateway and execution-core. **Pass the same environment as the A6 build** (`AGENTIC_SUBSCRIPTION_ENABLED=true`, `AGENTIC_SUBSCRIPTION_ALLOW_EFFECTS=false`, `AGENTIC_SUBSCRIPTION_MAX_ROUNDS=14`, `WIDE_RESEARCH_FAN_OUT_CONCURRENCY=2`, `EXECUTION_RUN_WALL_CLOCK_SECS=240`) if Do mode on Terra should stay on: the compose defaults are off, so a launcher run without them switches Do back off. H5 itself does not use Do mode.
3. **Free checks after the rebuild** (no Terra quota):

   ```
   # the new text is in the new binaries (both must print 1; before the rebuild: old=1 new=0)
   docker exec model-plane-model-gateway-1 sh -c 'grep -c "python-pptx, lxml" /usr/local/bin/model-gateway'
   docker exec model-plane-execution-core-1 sh -c 'grep -c "imported every library" /usr/local/bin/execution-core'

   # the row is fresh (health_checked_at within 60 s of now). capability-core keeps the reason CODE but replaces execution-core's reason text with its own canned "Capability health check succeeded.", so the new wording cannot be read from this row: the grep above is what shows the new build is running
   docker exec -i model-plane-postgres-1 sh -c 'psql -U "$POSTGRES_USER" -d session_core' <<'SQL'
   select id, availability_state, availability_reason_code, health_checked_at, now() from capabilities where id = 'cap.command.sandbox';
   SQL

   # no import failure at boot (this prints nothing when healthy)
   docker logs model-plane-execution-core-1 2>&1 | grep "code_interpreter runtime is not healthy"
   ```

   If the last command prints a line, its `missing_modules` names the library the image lacks; fix the image, not the probe.
4. **Oracle self-test and dry run** (no model, no quota; the dry run needs the localhost session to be signed in, check `GET localhost:3011/api/inference-core/token` with the stored cookies answers 200, and uses `--no-deps` so it does not run the sign-in setup):

   ```
   cd "apps/Frontend Plane/verevonv3"
   MSYS_NO_PATHCONV=1 docker run --rm --network none -v "C:/dev/CoresSystem/apps/Frontend Plane/verevonv3/tests/e2e/fixtures/code-interpreter:/h5:ro" --entrypoint python3 model-plane-execution-core /h5/h5_oracle.py self-test
   npx playwright test --project=product-readiness --no-deps tests/e2e/product-code-interpreter.spec.ts --grep "dry run"
   npx vitest run tests/code-interpreter-proof.test.ts src/features/chat/components/ChatMessages.generated-files.test.tsx
   ```

   `self-test` must print `"ok": true`. The dry run takes about 20 s.

## The live run

One case at a time, in this order, stopping at the first failure. Each is ONE Terra turn through `requireProductSubscription`, which blocks any request that is not the Terra subscription. They only run with `H5_LIVE=1`. Set `H5_PROOF_DIR` somewhere that is not cleared (Playwright clears `test-results` at the start of every run); the A6 proof used `C:\private\verevon-recordings\`.

```
cd "apps/Frontend Plane/verevonv3"
export H5_LIVE=1 H5_PROOF_DIR="C:/private/verevon-recordings/h5-proof"     # PowerShell: $env:H5_LIVE='1'; $env:H5_PROOF_DIR='C:/private/verevon-recordings/h5-proof'
npx playwright test --project=product-readiness --no-deps tests/e2e/product-code-interpreter.spec.ts --grep "live xlsx"
npx playwright test --project=product-readiness --no-deps tests/e2e/product-code-interpreter.spec.ts --grep "live docx"
npx playwright test --project=product-readiness --no-deps tests/e2e/product-code-interpreter.spec.ts --grep "live pdf"
npx playwright test --project=product-readiness --no-deps tests/e2e/product-code-interpreter.spec.ts --grep "live pptx"
npx playwright test --project=product-readiness --no-deps tests/e2e/product-code-interpreter.spec.ts --grep "live png"
```

If the oracle needs a local Python instead of docker: `H5_ORACLE_PYTHON=<python with openpyxl, python-docx, pypdf, python-pptx and Pillow>`. If the stack image has another name: `H5_ORACLE_IMAGE`.

The prompts are plain Norwegian and name neither the tool nor Python (in `tests/e2e/code-interpreter-proof.ts`). Whether Terra chooses the tool is part of the proof.

### Pass criteria (what `judgeCase` checks; every shortfall is listed at once)

For each case:

- the stream has no `error` frame and ends with `done` for `gpt-5.6-terra`;
- the tool decision called `code_interpreter` (the evidence line lists every tool called);
- one call exited 0 and delivered a file with the right extension, and every call returned inside 30 s (measured from the `tool_call` frame to its `tool_result` frame, so it includes dispatch);
- an `attachment` frame carried that file as a non-empty data URI whose size matches what it declared;
- the file is a download card with the data URI as its link (the PNG instead is an inline preview that loaded, and is not also a card);
- every control was clicked and a real download was saved;
- the oracle read the saved file with its library: **xlsx** (openpyxl) at least 4 formula cells including a `SUM`, a native bar chart part bound to cell ranges, header columns Omsetning, Kostnad and Margin; **docx** (python-docx) a table of at least 4 rows by 3 columns with the header Aktivitet, Ansvarlig, Frist, at least 3 paragraphs, the title Statusnotat; **pdf** (pypdf) the words Sammendrag, blåbær, grøt and Ålesund in the extracted text; **pptx** (python-pptx) two slides, a picture that is not blank, the title Kvartalsrapport; **png** (Pillow) PNG, at least 300x200, not blank;
- the answer does not link to the file (`sandbox:/` paths and markdown links resolve to nothing).

A first call that fails and a second that succeeds passes, and is visible in the evidence (`code_interpreter` has one entry per call; `tool_errors`; `traceback`).

### Evidence

One JSON line per case in `$H5_PROOF_DIR/evidence.jsonl`: ids, counts, timings, file names, 16-character hashes. No prompt, answer or token (every string passes `scrub`). The downloaded files (invented content) and the oracle's full verdict are in `$H5_PROOF_DIR/<kind>/`. Playwright also attaches the stream, delivery and oracle JSON to a failed test, and saves a screenshot and page snapshot of the chat (prompt and answer) under `test-results`; delete those folders before sharing a run.

To re-read a saved file by hand (from Git Bash, with `MSYS_NO_PATHCONV=1` and Windows-style absolute paths, as in the self-test line above): `docker run --rm --network none -v "<abs>/tests/e2e/fixtures/code-interpreter:/h5/oracle:ro" -v "<abs>/<proof dir>/<kind>:/h5/files:ro" --entrypoint python3 model-plane-execution-core /h5/oracle/h5_oracle.py check /h5/files`.

### When a case fails

| What the evidence says | Look at |
|---|---|
| `code_interpreter_calls` is 0 | The `tools` list says what was called instead (`create_artifact` for the Word document is the likely one, or nothing). Check `request_features` holds `tools` and not `conversation_only` (a request restricted to the conversation still offers code_interpreter, but a missing `tools` removes every builtin). The tool decision asks Terra with `tool_choice: required` among the offered tools plus the finish tool (`tool_loop.rs` ~L6667); read the inference-core log for the request id before changing anything. Do not widen the description or add a forcing rule until the log says which tool was chosen and why. |
| `error_codes` has `subscription_usage_limit` or `rate_limited` | The cap. Record it and stop. Do not retry on another route. |
| a call with `exit_code` not 0, or `traceback` true | The tool result carries the Python error; check whether the model repaired it in a later call. A missing module means the image and the description disagree (the guards should have caught it). `select tool, status, reserved_at, finalized_at from session_tool_audit_intents where request_id = '<request_id>'` in `session_core` shows which tools the gateway dispatched and how they ended. |
| `result_after_ms` near 30000 | The sandbox limit (`EXECUTION_CODE_TIMEOUT_SECS`). Look at what the program did; the reference programs take 0.1 to 1.1 s. |
| an attachment but no card or preview | The `attachment` frame arrived and the page did not render it: a frontend defect, since `GeneratedFiles` is pinned by its test. Check the page for the message and the delivery attachment. |
| the oracle rejects the file | Its `problems` are in `$H5_PROOF_DIR/<kind>` and the test's `-oracle` attachment. The oracle is strict only about what the prompt spelled out; a different chart type or an extra sheet does not fail. A PDF made by a library that does not write extractable text fails the word check: judge by opening it before calling it a model failure. |

## After a full pass

1. Add the dated record to `PRODUCT_RECORDING_READINESS_2026-09-24.md` as a new Q section (suggested "Q17"; the last used is Q16L). Template, to be filled from the evidence file (ids, counts and timings only, no answer text):

   ```
   ## Q17: code_interpreter on Terra, five Office outputs (<date>)

   Build: <model-gateway and execution-core image ids>, rebuilt through the launcher on <date>. Route: gpt-5.6-terra through openai-codex-subscription, every request through requireProductSubscription; no other provider.
   Cases (one fresh thread each): xlsx, docx, pdf, pptx, png. Evidence: <H5_PROOF_DIR>/evidence.jsonl.

   | Case | Tool called | Calls | Sandbox time (ms) | Delivered as | Oracle |
   |---|---|---|---|---|---|
   | xlsx | code_interpreter | <n> | <result_after_ms> | card | <formula cells>, <chart kind>, ok |
   | docx | ... | | | card | <table rows x columns>, ok |
   | pdf | ... | | | card | æøå present, ok |
   | pptx | ... | | | card | <pictures>, ok |
   | png | ... | | | inline preview | <width x height>, ok |

   Tool-decision rounds that picked another tool first: <none / which>. First-attempt failures repaired by the model: <none / which>. Cap hit: <no / when>.
   ```

2. Only after all five pass, promote `cap.command.sandbox` in `apps/Model Plane/docs/CAPABILITY_PROMOTION_LEDGER.tsv` (line 4) from `source_only` to `live`: `candidate_ref` the image ids, `live_evidence_ref` the Q section, `rollback_ref` the previous image ids, `expires_at` and `reviewer` per the ledger's rules. **Not done here**: there is no live evidence yet, and the ledger's own note says candidate, live and rollback evidence are absent.

## Findings to follow up (not fixed here)

- **Preview download name.** A PNG chart's inline preview downloads as `besokstall-png.png`, not `besokstall.png`: `generatedImageDownloadName` slugs the title, which is the artifact title (the file name with its extension), turning the dot into a hyphen. Seen in the dry run on the real page. It is in `chat-media-markdown.tsx`; strip a trailing image extension from the title before slugging. The proof does not depend on it (the oracle goes by extension).
- **Do mode already lists the libraries.** The Do-mode `code_interpreter` description (`execution-core/src/runtime_loop/agent.rs`, other sessions' uncommitted file) names python-pptx, XlsxWriter, pypdf and Pillow already, and not lxml; it was left alone. The new drift guard covers only the in-chat description.
- **The tool list is in every prompt, so run the saved journeys once before this build ships beyond the proof stack.** The new description is 556 characters (roughly 140 tokens) longer, on every tool round of every chat turn that offers the builtin tools (all of them, bar support threads). The Terra rounds measured in A6 carry about 10,200 input tokens each, so the cost is about 1.4 percent. The behavioural risk is that the longer text changes which tool Terra picks for the four journeys' documents (a Markdown report is `create_artifact`; the new text talks more about producing files). It is a tool description, not a system-prompt notice, so the A/B-on-the-matrix rule does not apply, but plan risk 4 does: one pass of journeys 01 to 04 on Terra, and no regression, before shipping. The unit suite cannot see this (the full model-gateway library suite passes with the new text: 1651 passed, 0 failed, 2 ignored).
