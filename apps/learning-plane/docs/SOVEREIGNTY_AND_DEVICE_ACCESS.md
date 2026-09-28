# Sovereignty and device access

Date: 2026-09-24. Default deployment profile: **strict-self-hosted**.

## Processing boundaries

Organization identities, lesson evidence, documents, screenshots, audio, transcripts, model requests, backups and telemetry remain inside the approved school/operator boundary. An EU endpoint alone does not prove full sovereignty; processor control, model hosting, network egress, logs, backups, key ownership and deletion behavior all require evidence.

Self-hosted Whisper recognition is selected. Whisper's speech-to-English translation capability is not a general Norwegian/English localization or narration service. Translation and TTS are separately configured self-hosted Model Plane capabilities. Downloading models/dependencies during development does not prove private runtime operation. Runtime egress tests and model artifact provenance are release gates.

The shared CoreSystem WhisperFlow service is planned under Control Plane: authenticated speech-session/job management and policy shared across Teachme, Verevon and future meetings. Model executes the selected private capabilities; Data stores retained content only when permitted. Control job/audit records carry references and minimal operational metadata, not raw audio or transcript bodies. Existing hosted dictation routes do not satisfy this deployment profile.

The foundation's policy gate rejects cloud-hosted or externally processing capabilities. It does not itself implement a network firewall. A future private deployment must combine server-resolved processors, egress denial, no fallback, content-free operational telemetry and ZDR propagation. Self-hosting speech does not make externally hosted Astra sovereign. A separate externally approved profile is a future design decision, not an enabled exception.

The user's existing subscription-only test restriction remains: no paid API/Claude/balanced-provider tests are introduced. This foundation makes no model calls. Strict inference needs a separately validated self-hosted model route before student use.

## Teacher device access

Three operations are distinct:

1. Authenticate the teacher to Teachme using Control.
2. Enroll a device with a school-bound device key, authenticated ownership and policy.
3. Join the learner's active, unlocked Windows session with a scoped assistance grant.

None of these means collecting student passwords, logging in as the student, bypassing the lock screen, automating UAC, or installing unattended privileged access. School IT sign-in/provisioning may be designed separately; it cannot be inferred from teacher status.

A future assistance grant binds org, class, teacher, learner, device, local session, purpose, issue/expiry time and exact permissions. Control verifies identity/membership/capability; Learning verifies class/assignment scope and stores assistance intent; the native agent verifies signed grants and local host acceptance, and enforces the current action scope. The supplemental Go policy module only checks already-verified evidence; it never verifies a signature or authenticates a request.

View, annotations, pointer input, keyboard input, clipboard and AI observation are separate grants. Default requests are view-only. Assistance must be visible on the student's device with immediate local stop. Revocation and stale-frame/window checks apply just before execution, including after asynchronous approvals. In-flight operations need device-side cancellation; browser middleware alone is insufficient.

The teacher overview initially contains progress and help requests. Live screen previews are an explicit authorized session feature. Neither the teacher title nor school membership enables hidden continuous monitoring.

## Existing remote implementation

Reuse the package at `apps/Frontend Plane/verevonv3/packages/remote-core`; keep Support Plane as transport. The shared permission recheck is improved in this pass. No relay, remote host or live student session is started.

The existing RustDesk-compatible protocol's documented nonce-reuse limitation and live-host interop gaps must be resolved or addressed through a reviewed protocol/transport decision before school remote access. Operating a relay yourself does not remove the cipher limitation. A viewer-side highlight is not a student-desktop annotation; native overlay and a versioned annotation channel still need implementation.
