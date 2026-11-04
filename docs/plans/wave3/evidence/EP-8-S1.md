# EP-8-S1 evidence: local date, time and app language in the prompt

Build status: partial. The code, the switch and the unit tests are written. The model-gateway crate does not build in this batch until the lead applies the shared-file lines below. The live proof is a lead step and is not claimed here.

## Switch

`MODEL_GATEWAY_USER_LOCALE_CONTEXT`, default off, read by the strict parser (only the literal `true` enables it). The BFF sends `client_timezone`, `client_utc_offset_minutes` and `ui_locale` on every turn; model-gateway reads them only while the switch is on. With the switch off the date notice is byte-identical to HEAD (golden test `locale_context_switch_off_notice_is_byte_identical`).

## Tests run in this batch

| Command | Result |
|---|---|
| Isolated harness: `model-gateway/src/locale_context.rs` compiled alone with `chrono =0.4.45`, `cargo test --offline`, `CARGO_TARGET_DIR=C:/dev/mp-target-w2` | 6 passed |
| `cargo test --manifest-path "apps/Frontend Plane/verevonv3/apps/gateway/Cargo.toml" --all-targets locale` | `locale_fields_are_validated_and_never_invented` and `locale_bogus_timezone_is_dropped` passed; exit 0 |
| `pnpm exec vitest run src/shared/api/chat-client.locale.test.ts` | 5 passed |
| `pnpm exec vitest run src/shared/api/chat-client src/features/chat/components/use-chat-controller src/features/dashboard/home/DashboardComposer.subscription.test.tsx src/features/inbox/lib/inbox-ai.test.ts` | 35 files, 449 tests passed (the last run; the first run had 34 files, and the match set can change while other batches add test files) |
| `node node_modules/typescript/bin/tsc6 -p tsconfig.app.json --noEmit` | one error, in `src/shared/api/schedule-contract.test.ts`, a file this slice does not touch; none in the chat client |
| `cargo test -q -p mp-contracts --test prompt_tool_manifest` | 9 passed. The pinned prompt files are untouched, so the manifest and its pin did not change and no eval case or replay cassette is required by the scenario-touch gate |
| `rustfmt --check` on `locale_context.rs` | clean |
| `rustfmt --check` on the six modified Rust files | no parse errors |

Not run in this batch: the model-gateway unit tests, `cargo test -q -p model-gateway --lib locale_context` and `--lib prompt_context`. The crate needs the `lib.rs` module line first.

## Lead steps

1. Apply the shared-file lines listed in the slice report (`lib.rs`, `zdr_features.rs`, `normalize.rs`, `json_handlers.rs`, `ag_ui.rs`, `docker-compose.yml`).
2. `cd "apps/Model Plane/rust" && CARGO_TARGET_DIR=C:/dev/mp-target-w2 CARGO_INCREMENTAL=0 cargo test -q -p model-gateway --lib locale_context`, then `--lib prompt_context`, then `cargo test -q -p model-gateway --test zdr_feature_table`.
3. Live proof (depth phase): rebuild model-gateway and the BFF, set the browser zone to `Pacific/Auckland`, ask for today's date and weekday when Auckland is a day ahead of UTC, and capture the rendered notice from the model-gateway debug log.

## Before the switch goes on

- The local time changes every minute and the date notice sits ahead of the history under the legacy prompt order, so prefix caching breaks on every turn. The owner decides the precision (minute, hour or date only) before the switch is turned on.
- Do runs receive the notice only when the run-context mode is on. Without it the Do preamble carries no date, which is today's behaviour.
