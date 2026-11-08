# EP-8-S8 evidence: personal first-run step

Build status: code and unit tests written, nothing run (code-first batch W3-09, owner order 2026-11-03). The guard compiles and runs the tests, and the lead applies the shared edits listed below. The live proof is a depth-phase step and is not claimed here.

## Switch

`PERSONAL_SETUP_VISIBLE` (user-core), default off. Read by `personalSetupVisible()` with the strict parser (only the literal `true` enables it). Off: `personalSetupAvailable` is `false` on the AI settings response, so the card never renders. The stored state is still readable and writable, and nothing else reads it.

## Stored state

User-core settings category `onboarding`: `{personalStep: pending | completed | skipped, version: 1, updatedAt}`.

- `GET` and `PUT /api/v1/settings/onboarding`. The gateway's generic `/api/v1/settings/{key}` proxy already forwards both, so the card needs no new gateway route for reads and writes.
- The client sends `personalStep` only. `version` and `updatedAt` are stamped on the server. An open value is refused with `personal_step_invalid`, whose `details.allowed` carries the closed set.
- A stored value outside the closed set reads as `pending`, never `completed`.
- No migration (JSON settings category). The organization onboarding state (`/api/v1/users/me/onboarding-state`, session `onboarding_complete`) is unchanged and is a different store.

## Actions

- `settings.complete_personal_setup`. Input: `displayName?` (at most 80 characters), `timezone?`, `language?` (`nb-NO` or `en-US`), `tonePreset?` (closed set). The gateway checks the input, then writes in this order: preferences (`timezone`, `language`), the name (`PATCH /api/v1/users/me`), the reply tone (GET then PUT of `/api/v1/settings/ai`, as `settings.set_tone_preset` does), and last the onboarding state `completed`. A failed write stops the step and leaves it `pending`, so a retry is safe.
- `settings.skip_personal_setup`. Input `{}`. Writes `skipped`.
- Both are human-only rows in `model-access.ts` (`exempt('human_identity_or_security', 'external_write')`), `eligible: false`, so no model can execute them.
- Registry count: 255. Counted before this slice as 207 top-level entries plus 46 Space fragment entries (253, which includes EP-8-S4's two profile contracts), plus this slice's two additions. The P-SPA1 count test still said 251 after EP-8-S3; EP-8-S4 added two contracts without moving it.

## Role memory

After the answers are saved, the card saves a stated role with `rememberMemory(threadId, "Role: ...")` (REST `POST /api/v1/memory`), and only when all of these hold: the card has an open thread, `GET /api/v1/memory/controls` reports `state: on` with `generationAllowed: true`, and the person ticked the box. Paused, off, org-disabled and ZDR memory hide the box. A failed role save keeps the answers and shows a notice.

Open: the empty chat has no thread on first run (threads start on the first send), so the box does not render there. See the depth findings.

## Card

`PersonalSetupCard.tsx`, mounted above the empty chat by a shared edit in `ChatPage.tsx`. It is offered when `available` is true and `step` is `pending`. Any failed read hides it. Skip records `skipped` and hides the card; a saved step hides it. It never blocks the composer. Strings use `useI18n().tr`, with Norwegian first.

## Files

Edited (owned): `apps/Control Plane/user-core/internal/http/handlers.go`, `apps/Frontend Plane/verevonv3/src/shared/actions/action-registry.ts`, `apps/Frontend Plane/verevonv3/src/shared/actions/model-access.ts`, `apps/Frontend Plane/verevonv3/src/shared/actions/action-registry.test.ts` (test beside the registry).

New: `apps/Control Plane/user-core/internal/http/onboarding_settings_test.go`, `apps/Frontend Plane/verevonv3/src/shared/api/personal-setup-client.ts`, `apps/Frontend Plane/verevonv3/src/features/chat/components/PersonalSetupCard.tsx`, `apps/Frontend Plane/verevonv3/src/features/chat/components/PersonalSetupCard.test.tsx`, `apps/Frontend Plane/verevonv3/src/features/chat/components/personal-setup-card.css`, this file.

## Tests written, not run

- Go, `apps/Control Plane/user-core`: `go test ./internal/http/...`. New tests: `TestOnboardingCategoryClosedValues`, `TestOnboardingPatchStampsVersionAndTime`, `TestOnboardingDefaultsArePendingAtVersionOne`, `TestOnboardingReadNormalisesUnknownStepToPending`, `TestUpdateOnboardingRefusesOpenStepWithTypedEnvelope`, `TestPersonalSetupVisibleIsOptIn`, `TestAISettingsResponseCarriesPersonalSetupFlag`, `TestAISettingsPatchNeverStoresPersonalSetupFlag`.
- SPA, `apps/Frontend Plane/verevonv3`: `pnpm exec vitest run src/features/chat/components/PersonalSetupCard.test.tsx src/shared/actions`. The card tests cover: hidden when not offered, when completed or skipped, and when the read fails; the four answers sent through the complete action; a display name over 80 characters refused before any write; skip recorded and kept hidden; role hidden while memory is paused and without a thread; role saved after the answers; role failure keeps the answers and says so. The registry count test expects 255.

## Deviations from the design text

- Section 4, S8 of the design says PATCH for the onboarding category. The route is PUT, as every other settings category is.
- `personal-setup-client.ts` and `personal-setup-card.css` are new files outside `new_files`, as EP-8-S3 did with `ai-settings-client.ts`.
- `settings.complete_personal_setup` makes four gateway writes rather than calling one user-core endpoint. The design names the same four writes.
- The role save runs from the card after the action, not inside it, because it needs the open thread and the memory gate.

## Live proof (depth phase, not claimed)

1. Fresh account with the switch on: the card appears once above the empty chat, at 375 px and 1280 px (screenshots).
2. Complete with Direct and `Pacific/Auckland`: `GET /api/v1/settings/onboarding` returns `completed`, and the next turn's composed prompt shows the tone and the local date (EP-8-S1 and EP-8-S3).
3. Skip on a second account: the card does not return after a reload.
4. `PUT /api/v1/settings/onboarding` with `{"personalStep":"done"}` returns 400 `personal_step_invalid` with `details.allowed`.
5. Switch off: no card, and `GET /api/v1/settings/ai` differs only by `personalSetupAvailable: false`.
