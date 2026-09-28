# Learning Plane / Teachme implementation status

Date: 2026-09-24. This records code and evidence, not a pilot-readiness claim.

## Current state

L00 foundation work remains in place. This pass implements the main L01 school/course source path:

- Learning Core can stay closed in unconfigured foundation mode, or require all four Control/database settings before enabling authenticated operations.
- Learning authenticates the current Control session, verifies the active organization membership through Auth Core's exact-membership decision, and ignores client-supplied identity, organization and role headers.
- Auth Core accepts a distinct optional Learning-only membership credential, so existing deployments remain bootable while Learning is unprovisioned.
- Learning-owned PostgreSQL migrations create RLS-protected tenant classes, class members, courses, immutable published course revisions, revision-pinned assignments, attempts and ordered attempt events. Runtime uses a limited app DB role; attempt events are append-only and attempt status has column-limited update rights.
- The API and Solid dashboard source support existing-account sign-in, class setup, verified member assignment/removal, constrained lesson-definition drafts, publish, assignment views and teacher-only attempt summaries. Attempt/event creation is idempotent, learner-scoped and pinned to the assigned revision. The dashboard request marker blocks cross-origin browser writes in the absence of permissive CORS.
- Completion events are explicitly learner-reported; no attempt is represented as independently verified mastery. Event records contain no free-form learner text, audio or screen evidence.
- Foundation-mode and configured-mode status are both accepted as distinct states; the public status never implies tutoring or device guidance is ready.

## L01 gaps and live gates

L01 is **not complete**. Source alone does not prove the system works without errors in a real session. The following remain open:

- No product-entitlement check or dedicated Learn school provisioning workflow; any active Control organization membership can enter the Learning path if the service is enabled.
- Local Control Auth, Learning database and fully configured Learning Core have not been brought up together in this pass. No real sign-in, class creation, member check, course publish or student access has been exercised.
- Cross-school, other-class, removed-member, expired-session, malformed-body, role-transition, concurrency and restart/retry isolation scenarios have not been run against disposable records.
- Control audit integration/outbox and Application notification integration are pending. Attempt and school/course mutation events currently remain only in Learning storage.
- Course source-version references are pending. There is no student-facing attempt flow or native lesson integration yet. Explicit `Idempotency-Key` handling currently covers attempt creation and attempt events; it has not been generalized to all mutations.
- Current member entry uses Control user IDs; there is no school directory, invitation/import, organization switcher or account recovery UX.
- No real school/student accounts or data have been accessed.

## Validation evidence and limits

The Learning Core row reflects this attempt/progress pass and a fresh disposable PostgreSQL database. The other rows are earlier L00 evidence, not proof of the full student journey.

| Area | Evidence | Limit |
|---|---|---|
| Learning Core | `go test ./...` and `go vet ./...` passed; both migrations applied to a fresh disposable PostgreSQL instance; synthetic RLS, app-role privilege, assigned-step lookup and teacher progress queries returned expected results | No live Control session, authenticated HTTP attempt journey, or school-tenant isolation/revocation acceptance journey was run |
| Shared remote-core | Focused `ActionExecutor` tests and package typecheck passed | Cipher/transport and live remote correctness remain unresolved |
| Learn clients | `pnpm typecheck`, `pnpm test` and `pnpm build` passed after the teacher progress view and status capability changes | No authenticated student attempt or native guidance journey has run |
| Windows client | .NET 10 Release build and WPF resource smoke check passed | No device capture, overlay, hotkey interaction, enrollment or signed installer |
| Local previews | Marketing/dashboard responded; foundation status and Go liveness were observed | This is not an authenticated Learning operation |

## Remaining product scope

The first Windows lesson still needs device enrollment, selected-window UI Automation/capture, DPI-aware annotation, local target revalidation and safe stop. The attempt API records learner-reported events only; verified outcomes and independent practice evidence are absent. Whisper recognition, separate translation/TTS, private tutoring and teacher device assistance remain disconnected. Remote-core's documented cipher/transport limits remain unresolved. No Whisper model or cloud provider has been invoked. No school deployment or signed installer exists.

## Next work

Finish L01 before starting a student-data pilot: resolve and configure Learn entitlement/provisioning; provision a disposable school tenant; configure Control's dedicated Learning credential and trusted dashboard origin; start isolated Learning Postgres and configured Learning Core; validate teacher/student journeys and negative isolation/revocation cases. Before audit or notification publication, provision the exact Control Audit producer and Application source contracts, then add transactional outbox and retries. Use synthetic data only. Record all results and failures. The student-facing attempt flow must follow the Windows lesson integration; the current teacher summary is only a projection of learner-reported events. After L01 passes, implement L02's single teacher-reviewed PowerPoint lesson. The shared WhisperFlow contract stays planned for L03; this pass does not install or connect it.

See the [structured implementation plan](IMPLEMENTATION_PLAN.md) and the [Learn Core workspace instructions](../../Frontend%20Plane/learn-core/README.md).
