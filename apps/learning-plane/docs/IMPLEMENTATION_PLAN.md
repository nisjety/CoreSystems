# Teachme structured implementation plan

Date: 2026-09-24. This file describes implementation work, not a product-readiness claim.

## Product and pilot

Teachme is the first B2B learning product on CoreSystem. Pilot: one school, ages 13–18, Windows 11, one supported PowerPoint version and three teacher-reviewed lessons. Language targets: Norwegian and English. School roles: instructor/learner; backend uses organization/group/author/learner so Verevon can later reuse it for employees.

User experience: keyboard shortcut -> type a request -> explain one step -> point at a verified target -> student acts -> inspect the outcome -> record evidence of progress. Modes are guide, demonstrate and practice. An AI-completed step must not count as independently demonstrated student mastery.

## Stack and workspace

| Surface | Choice | Initial scope |
|---|---|---|
| Marketing | Next.js / React | Product introduction and honest pilot status; no fabricated customers, download claims or signup submissions |
| Dashboard | Vite / Solid 2 | Instructor workspace, connection/readiness view and curriculum planning shell |
| Windows | C# / .NET 10 / WPF | Native request window and configurable-in-code shortcut; future native capture/UI Automation/overlays |
| Learning backend | Go, one service | Domain/policy modules and HTTP contracts; Postgres persistence next |
| Shared speech system | CoreSystem WhisperFlow, working name | Control Plane service API, access policy and job metadata; Model Plane executes self-hosted Whisper; Data owns retained content |
| Translation / narration | Separate self-hosted capabilities | No claim that Whisper is arbitrary-language translation or TTS |
| Remote guidance | Existing remote-core plus native adapter | No parallel RustDesk client implementation |

## Delivery passes

### Sequencing clarification — shared WhisperFlow

The user selected a CoreSystem-owned WhisperFlow system inside Control Plane, reusable for Teachme, Verevon and later meeting summaries. Interpret “Control Core” as the existing Control Plane. Proposed service name: `speech-core`; its folder/runtime has not been created. Control owns its authenticated service API, speech-session/job metadata, processing permissions, quotas and audit integration. Model retains transcription/translation/narration/summary execution; Data retains approved audio/transcript/summary artifacts through its APIs. Product UI and meeting collaboration remain Frontend/Application concerns. No transcript bodies in Control audit logs or duplicate knowledge database.

Define the shared contract during L01: scoped session/job IDs, use case, language, partial/final transcript events with sequence numbers and timestamps, cancellation, processing/retention profile and typed failures. Server-resolved policy determines processors; clients cannot widen processing permission. Teachme initially consumes the same transcript interface with typed input available while speech is disconnected.

Delivery order: (1) school access and saved assignments, (2) one complete typed lesson on Windows, (3) shared speech with self-hosted Whisper, (4) teacher progress/help and authorized remote assistance, (5) pilot hardening. Speech contract design can proceed alongside L01; a full meeting product is not a dependency of the first lesson. Later meeting work adds long recordings, speaker attribution, resumable chunks, source-linked summaries/decisions/actions and reviewed retention. Summary generation is a separate operation from recognition.

Reuse candidates inspected in source: Verevon's `src/shared/api/audio-client.ts`, Model Gateway's `/v1/ai/speech` and `/v1/ai/dictate`, and inference-core's `src/provider/speech.rs`. These currently include hosted-provider routing; existing dictation is not evidence of a working sovereign Whisper service. Preserve existing product behavior until a separately tested migration is ready.

### L00 — Foundation (verified; integrations pending)

- [x] Architecture, ownership, sovereignty/device policy and this plan.
- [x] Loopback-only Go service with distinct liveness and blocked readiness.
- [x] Supplemental policy checks for tenant/class/device/grant scope, expiry, revocation, action mode and strict processing.
- [x] Next.js marketing and Solid 2 dashboard source with shared typed status client.
- [x] Native C# request-window source with global shortcut, explicit inactive capture/control state and Escape dismissal.
- [x] Shared remote-core permission recheck immediately before dispatch after asynchronous middleware.
- [x] Complete build/test evidence and update [STATUS.md](STATUS.md); foundation checks pass, with live teaching and school-readiness gates explicitly open.

### L01 — Trusted school access and durable learning records

**Source status (2026-09-24): core of steps 1–3 is implemented, but the L01 exit gate is still open.** Learning Core now has an optional Control session and exact-membership adapter, dedicated membership credential, tenant-scoped PostgreSQL schema/repository, class/member/course/revision/publish/assignment APIs, attempt/event APIs and a teacher progress summary in the dashboard. Attempt events are learner-reported, revision-pinned and idempotent; they do not establish verified mastery. The implementation currently treats an active Control organization as the pilot school; Learn entitlement/organization provisioning has not been integrated. There is no Control audit/outbox or Application notification delivery, source-version references, student-facing attempt flow, real-school acceptance or executed isolation/recovery evidence.

1. **Implemented in source / live gate pending:** reuse the current Control session and exact canonical membership decision; Learning has its own dedicated membership credential. There is no second password database. Add Learn product entitlement/organization provisioning, confirm production audience/scope requirements and verify the trusted-origin/session setup with a real disposable school tenant.
2. **Partially implemented:** Learning-owned Postgres stores tenant-scoped classes, class members, course metadata and immutable published revisions, plus class assignments pinned to one revision. App DB access uses a least-privileged role and RLS. Attempts pin to assignment revisions; ordered events have SHA-256 idempotency keys and append-only database privileges. Source-version references, audit outbox and retry processing remain pending.
3. **Implemented in source / negative validation pending:** class teacher/student checks narrow verified Control organization membership. Attempt writes require the active student to own the attempt and class; lesson step IDs are checked against the assigned published revision. The UI writes only through the authenticated Learning API. Prove cross-school, other-class, role-change, removed-member and expired-session negatives against disposable records before passing this step.
4. **Pending:** integrate Control audit and Application notifications using their actual versioned contracts and configured Learning principals. Add durable transactional outbox/retry semantics for meaningful events.
5. **Pending:** prove cross-school, other-class, removed-member and expired-session negatives, plus restart/retry durability with disposable integration data.

Exit: a real authorized teacher can publish a lesson and assign it; only assigned learners can access it. **Not passed.** Source compilation does not prove the live Control session, membership credential, school entitlement, tenant isolation, audit, or durability behavior. No real child data before school deployment controls are reviewed.

### L02 — Native local guidance

1. Enroll a school-managed device using a device-bound key held in Windows-protected storage. Bind device, organization and current Windows session without storing the student's password.
2. Implement selected-window capture and UI Automation; mask sensitive controls before any permitted model request. Prefer semantic control identities; screenshots are bounded fallback observations.
3. Add a click-through annotation window with native DPI/display mapping, readable captions and an optional teaching pointer. Verify on the student's actual desktop, not only an instructor's remote canvas.
4. Define observation IDs, expiry, window/display identity, bounded actions and execution receipts. Device revalidates permissions and target freshness at execution; stop releases held keys/buttons and cancels queued actions.
5. Connect Model's step proposal -> local policy -> execution/annotation -> new observation. Use Quarry for browser actions. No arbitrary model-generated code on student desktops.
6. Validate one PowerPoint task at 100/125/150/200% scaling, window moves, keyboard layouts, wrong clicks, focus changes, disconnected services and immediate stop.

Exit: a student completes a lesson, and a second exercise establishes independent learning rather than AI task completion.

### L03 — Shared WhisperFlow, sovereign speech and reasoning

1. Benchmark self-hosted Whisper on school hardware/server with Norwegian/English examples. Record model origin/license/hash, CPU/GPU footprint and word-error/latency results.
2. Implement Control's shared WhisperFlow service contract and a Model Plane speech adapter with private endpoints, cancellation and no cloud fallback. Reuse existing authority and inference contracts. Audio stays ephemeral by default; approved persistence uses Data APIs and follows the school's explicit policy. Prove one Teachme request and one independent consumer use the same contract.
3. Select separate self-hosted translation/TTS engines and validate pronunciation, accessibility and captions matching narration.
4. Select/evaluate a self-hosted vision/reasoning model for strict deployments. Astra may be evaluated only in a separately authorized external-processing environment with synthetic/adult test data; it is not sovereign inference.
5. Verify egress denial, error behavior when a model is unavailable, deletion propagation and supply-chain/model artifacts. ZDR must be enforced across every processing boundary.

Exit: a complete lesson uses only approved local/private processing. No fallback to another provider to make a demo appear successful.

### L04 — Teacher assistance and class progress

1. Resolve remote-core's documented protocol/security release gates and prove a live Windows host/relay session; do not treat mocked peers as live proof.
2. Add durable assistance requests/grants tied to teacher, learner, org, class, device, current session, purpose, allowed actions and short expiry. Capture visible device acceptance; investigate institutional consent separately with the school.
3. Separate view, annotate, input and AI-screen-processing permissions. Default view-only requests. Teacher role does not grant arbitrary device access.
4. Add learner-visible active-assistance state, local stop, teacher handback, immediate revocation and content-minimal audit receipts.
5. Class dashboard shows authorized attempt/progress events. Live thumbnails require separate active screen-sharing authorization; no background classroom screen wall.
6. Distinguish Teachme authentication, device enrollment and joining an unlocked Windows user session. Windows sign-in/UAC/secure desktop remain a separate school-IT design, outside these teaching grants.

Exit: a teacher can assist a consenting enrolled learner and loses access immediately when scope/permission is revoked.

### L05 — Courses that remain current

- Author/review/publish lifecycle. Store semantic goals, preconditions, expected results, source versions, app versions and recovery hints.
- Layout changes: re-observe and locate equivalent controls. Workflow/policy changes: mark affected revisions for review and create a draft, never silently rewrite a published course.
- Teacher demonstrations produce suggested steps from approved evidence. Model proposes; author validates in a synthetic sandbox and publishes.
- Attempts remain pinned to their assigned course revision. Completion requires verified outcomes; retries cannot duplicate rewards/progress.
- Maintain compatibility matrix and regression lessons per application version. An uncertain target pauses guidance and offers a clear recovery path.

### L06 — Pilot evidence and later products

Pilot gate: instructor/learner usability sessions; correctness and independent practice outcomes; measured first-guidance/step latency; device stop/revocation; tenant isolation; service restart recovery; accessibility; school-approved retention/egress; signed installer/update/rollback; classroom-network testing. Set numeric budgets from L02/L03 measurements before pilot acceptance. No claim of universal or flawless application support.

Then integrate Learning APIs/SDK into Verevon for employee onboarding and company-source courses. Company terminology and policies differ, but backend domain/desktop contracts remain shared. Agency and macOS are later product phases.

## Contract inventory to implement

| Contract | Owner | Status |
|---|---|---|
| Public foundation status / health | Learning | Implemented |
| Canonical principal and exact organization membership | Control -> Learning | Source integration implemented; requires dedicated credential and live-session acceptance |
| Learn entitlement / school provisioning | Control -> Learning | Pending |
| Course/revision/assignment API | Learning | Source implementation present; integration validation pending |
| Attempt/progress API | Learning | Source API and teacher summary implemented; student lesson client, verified outcomes and live isolation checks pending |
| Source-version references and invalidation | Data -> Learning | Pending |
| Tutor requests | Learning -> Model | Pending; strict private routing required |
| Shared WhisperFlow session/job and transcript contract | Product clients -> Control -> Model; retained content -> Data | Proposed; no service runtime or private speech adapter yet |
| Teaching-assistance authorization + receipt | Learning, Control, native host | Pending |
| Native observation/annotation/action channel | Frontend shared desktop contracts | Pending |
| Remote transport | remote-core / Support | Existing implementation; live release gates open |

## Sources and limits

- [Clicky](https://github.com/farzaa/clicky): interaction reference; no source copied in this foundation.
- [Solid releases](https://github.com/solidjs/solid/releases): Solid 2 RC is pinned intentionally.
- [Whisper](https://github.com/openai/whisper), [whisper.cpp](https://github.com/ggml-org/whisper.cpp): self-hosted recognition candidates; arbitrary translation and narration require other components.
- [Wispr Flow privacy](https://wisprflow.ai/data-controls): cloud processing, not selected by the user.
- [Remote architecture](../../Frontend%20Plane/verevonv3/packages/remote-core/docs/architecture.md), [security](../../Frontend%20Plane/verevonv3/packages/remote-core/docs/security.md): known gaps remain binding.
