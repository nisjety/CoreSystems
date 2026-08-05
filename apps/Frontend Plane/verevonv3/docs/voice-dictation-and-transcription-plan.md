# Verevon Voice Dictation and Transcription Plan

**Date**: 2026-07-02
**Status**: proposed
**Scope**: Verevon v3, verevon-gateway-rs, Model Plane, Control Plane, Ingestion Plane, Data Plane

## Executive Summary

Verevon should implement a Wispr Flow-inspired voice layer, not a clone of
Wispr Flow's code, brand, or private product implementation. The product
pattern worth adopting is fast dictation into work surfaces, AI cleanup,
voice commands, vocabulary personalization, and enterprise-enforced privacy
controls.

The system must support two quality/privacy families:

1. High-quality cloud transcription with Zero Data Retention controls.
2. Lower-quality private transcription where raw audio and raw transcripts do
   not leave the customer's controlled infrastructure and do not enter the
   company database unless explicitly saved.

Verevon v3 already has a browser speech modal in
`src/features/dashboard/home/DashboardComposer.tsx`. The target is to replace
that browser-only path with a policy-aware CoreSystem voice stack.

## Reference Product Reality

Wispr Flow is primarily a universal dictation product: it works in text fields,
transcribes in real time, applies AI commands/edits, and learns vocabulary.
Its public docs describe Privacy Mode and Cloud Sync as separate controls; the
combination of Privacy Mode on and Cloud Sync off is their Zero Data Retention
mode for audio/transcript data.

Sources:

- [What is Flow](https://docs.wisprflow.ai/articles/2772472373-what-is-flow)
- [Data Controls](https://wisprflow.ai/data-controls)
- [Security and compliance FAQ](https://docs.wisprflow.ai/articles/3467817258-security-and-compliance-faq)
- [Wispr Flow vs Otter](https://wisprflow.ai/post/wispr-flow-vs-otter-december-2025)
- [Microsoft Teams transcript APIs](https://learn.microsoft.com/en-us/microsoftteams/platform/graph-api/meeting-transcripts/overview-transcripts)
- [Microsoft Graph transcript change notifications](https://learn.microsoft.com/en-us/graph/teams-changenotifications-callrecording-and-calltranscript)
- [whisper.cpp](https://github.com/ggml-org/whisper.cpp)
- [faster-whisper](https://github.com/SYSTRAN/faster-whisper)
- [Vosk](https://github.com/alphacep/vosk-api)

## Competitive Comparison Matrix

| Capability | Wispr Flow pattern | Verevon current | Verevon target |
|---|---|---|---|
| Universal dictation | Dictates into any app/text field | Dashboard composer only | Composer, chat, inbox, notes, CRM/ticket reply fields |
| Real-time transcription | Cloud dictation pipeline | Browser Web Speech API fallback | Model Plane STT plus local/private STT provider |
| AI cleanup | Punctuation, paragraphing, grammar, tone | None in voice path | `voice.format` pass through Model Plane |
| Voice commands | Spoken edits and transformations | None | Command mode with explicit command/dictation classification |
| Context awareness | Surrounding text helps style and continuation | Composer has local message context | Send bounded text context with policy filtering |
| Personal dictionary | Synced vocabulary | None | User/org dictionaries in settings, not transcript history |
| Snippets/prompts | Reusable text assets | General settings only | Voice snippets and command templates in settings |
| Enterprise privacy | Admin-enforced Privacy Mode and Cloud Sync | ZDR propagation planned | Org-enforced voice modes and retention policies |
| Meeting workflow | Adjacent follow-up workflow, not primary meeting archive | No dedicated Teams path | Teams transcript import, ephemeral analysis, explicit save |
| Local/private option | Public docs position transcription as cloud | Browser local only, weak control | Self-hosted `voice-local-core` using Whisper/Vosk |

## Core Product Modes

| Mode | Quality | Data boundary | Storage default | Intended customer |
|---|---:|---|---|---|
| `cloud_zdr` | High | May call approved cloud STT/LLM providers | No raw audio, transcript, prompt, or rewrite persistence | Teams that allow approved processors |
| `company_private` | Medium | Customer VPC/on-prem/private cluster only | No raw audio or transcript DB writes; memory/temp only | Regulated customers |
| `browser_basic` | Variable | Browser/runtime behavior | Browser only; no Verevon server processing | Demo and degraded fallback |
| `saved_knowledge` | Depends on upstream mode | Same as selected mode until save | User-approved summary/task/note only | Explicit knowledge capture |

`company_private` is intentionally lower quality at first. It should optimize
for privacy, auditability, and deployment simplicity before matching cloud
accuracy.

## Cross-Plane Ownership

| Concern | Owner | Notes |
|---|---|---|
| Voice UX, dictation controls, recording state | Frontend Plane / Verevon v3 | SolidJS feature module under `src/features/voice` |
| Browser-to-service proxy, ZDR propagation | verevon-gateway-rs | Browser calls gateway only |
| STT/TTS provider routing and cleanup prompts | Model Plane | `model-gateway` and `inference-core` |
| Org policy, allowed modes, retention defaults | Control Plane | user/org settings and entitlements |
| Teams transcript permission and Graph webhooks | Ingestion Plane / integration-corev2 | Microsoft Graph access and sync job state |
| Durable saved summaries/tasks/notes | Data/Application Plane by target object | Raw transcript must not be saved by default |
| Audit metadata | Control/Application audit path | Metadata only, never raw content under ZDR |

## Target User Workflows

### 1. Dictate Into Composer

1. User presses the voice button in the dashboard composer.
2. Frontend reads org voice policy from the gateway.
3. User records audio with clear active-recording UI.
4. Audio is sent to `POST /api/v1/voice/transcribe` or the streaming route.
5. Gateway injects identity, org, selected mode, and `x-zdr`.
6. Model Plane returns raw transcript or formatted transcript.
7. User inserts, edits, discards, or sends.

### 2. Voice Command

1. User speaks a command such as "make this shorter" or "turn this into a
   customer follow-up."
2. Model Plane classifies the input as command vs dictation.
3. The command applies only to the bounded text selection/context.
4. The UI previews the diff before replacing text.

### 3. Teams Transcript Follow-Up

1. Admin connects Microsoft tenant and grants transcript permissions.
2. integration-corev2 receives transcript availability notifications or a user
   starts a manual import.
3. Raw transcript is fetched from Microsoft Graph into ephemeral processing.
4. Model Plane produces a summary, action items, CRM note, and follow-up draft.
5. The user chooses what to save. Raw transcript is discarded unless org policy
   explicitly allows storage.

Microsoft Graph supports post-meeting transcript retrieval and transcript
change notifications, but access depends on tenant admin controls, application
permissions, and/or resource-specific consent.

## Target Gateway Routes

All browser calls go through verevon-gateway-rs.

| Gateway route | Upstream | Persistence rule |
|---|---|---|
| `POST /api/v1/voice/transcribe` | Model Plane `/v1/ai/speech` operation `stt` | No raw audio or transcript persistence under ZDR |
| `POST /api/v1/voice/transcribe/stream` | Model Plane streaming STT target | No buffered transcript replay under ZDR |
| `POST /api/v1/voice/format` | Model Plane `/v1/invoke` profile `voice_format` | No prompt/cache/session write under ZDR |
| `POST /api/v1/voice/commands/interpret` | Model Plane `/v1/invoke` profile `voice_command` | No durable command text under ZDR |
| `GET /api/v1/voice/policy` | user-core/org-core policy read | Metadata only |
| `GET /api/v1/voice/dictionary` | user-core settings | User-authored vocabulary only |
| `PUT /api/v1/voice/dictionary` | user-core settings | User-authored vocabulary only |
| `GET /api/v1/voice/snippets` | user-core settings | User-authored snippets only |
| `PUT /api/v1/voice/snippets` | user-core settings | User-authored snippets only |
| `POST /api/v1/voice/teams/transcripts/import` | integration-corev2 + Model Plane | Raw transcript ephemeral unless explicitly saved |
| `GET /api/v1/voice/teams/imports/:job_id/events` | integration-corev2 sync-job events | Progress metadata only |

## Model Plane Requirements

1. Extend speech contracts for streaming or chunked STT.
2. Add a `company_private` provider hint that routes to `voice-local-core`.
3. Add provider metadata: `provider_used`, `model_used`, `retention_mode`,
   `residency`, `quality_tier`, and `zdr_enforced`.
4. Add cleanup profiles:
   - `dictation_clean`: punctuation, paragraphs, filler removal.
   - `business_reply`: concise, customer-safe tone.
   - `technical`: preserve identifiers, code terms, and commands.
   - `meeting_followup`: action items, decisions, blockers, owners.
5. Disable prompt cache, transcript storage, session transcript writes, and raw
   event payloads whenever `zdr=true`.
6. Emit cost/latency metrics without audio or transcript content.

## Local/Private Provider Plan

`voice-local-core` should run in the customer's controlled environment and
expose the same STT contract as cloud providers.

| Engine | Role | Pros | Cons |
|---|---|---|---|
| `whisper.cpp` | Default private engine | Simple deployment, CPU/GPU support, no Python required | Lower throughput on small CPUs, limited diarization |
| `faster-whisper` | Higher private throughput | Better throughput with CTranslate2 and quantization | More deployment dependencies |
| Vosk | Lightweight fallback | Offline, low resource, many languages | Lower quality and weaker formatting |

Initial private default: `whisper.cpp` tiny/base/small models, with
`faster-whisper` as an optional accelerated deployment.

## Privacy and Retention Rules

1. Raw audio is never written to Postgres, Convex, Qdrant, Quickwit, MinIO, or
   durable NATS payloads under `cloud_zdr` or `company_private`.
2. Raw transcript is never written to a company database unless the user or org
   explicitly chooses a durable saved artifact mode.
3. Logs must include request ID, org ID, mode, provider, duration, and error
   category only.
4. Audit events must be metadata-only under ZDR.
5. Context awareness must send only bounded text context and must strip secrets
   where possible before provider calls.
6. Feedback/reporting is an explicit exception path and must warn users that
   submitted samples can be reviewed.

## Verevon v3 UI Plan

Create `src/features/voice` with:

- `voice-client.ts`: gateway API client.
- `voice-policy.ts`: mode selection and policy resolution.
- `voice-recorder.ts`: microphone capture, chunks, cancellation, timers.
- `VoiceComposerModal.tsx`: replacement for the current inline modal.
- `VoicePrivacyBadge.tsx`: mode indicator and retention explanation.
- `VoiceDictionarySettings.tsx`: vocabulary and snippets.
- `TeamsTranscriptImportPanel.tsx`: meeting import and summarize flow.

Update existing surfaces:

- `DashboardComposer.tsx`: replace browser-only `SpeechRecognition` path with
  the policy-aware voice feature.
- `PrivacyDataSection.tsx`: add voice retention, local/private mode, dictionary,
  and Teams transcript handling controls.
- Settings navigation: add "Voice and transcription" under privacy/AI settings.

## Delivery Phases

### Phase 0 - Decision and Risk Closure

- Confirm cloud provider processors and ZDR contractual posture.
- Decide whether `voice-local-core` belongs in Model Plane deploy or customer
  sidecar deploy.
- Decide whether Teams raw transcript can ever be saved.

### Phase 1 - v3 Dictation MVP

- Add gateway `voice/transcribe`.
- Use existing Model Plane `/v1/ai/speech` STT path.
- Add UI mode indicator and discard/insert flow.
- Tests: route envelope, ZDR header propagation, no content logging fixture.

### Phase 2 - Smart Formatting and Commands

- Add `voice_format` and `voice_command` Model Plane profiles.
- Add context-aware continuation with bounded text context.
- Add preview before replacement.
- Tests: command classification, prompt-cache bypass under ZDR.

### Phase 3 - Enterprise Policy

- Add org/user voice policy to Control Plane settings.
- Enforce allowed modes in gateway, not the SPA.
- Add admin controls for default mode and disabled cloud transcription.
- Tests: org policy denial matrix and metadata-only audit.

### Phase 4 - Company-Private Provider

- Add `voice-local-core` with `whisper.cpp`.
- Add optional `faster-whisper` deployment profile.
- Route `company_private` provider hint to local provider.
- Tests: network-egress-blocked smoke test, local provider health, WER sample.

### Phase 5 - Teams Transcript Import

- Add Microsoft Graph transcript import jobs in integration-corev2.
- Add transcript-available notification subscription handling.
- Add ephemeral summarization flow and explicit save.
- Tests: Graph fixture parse, no raw transcript persistence, save-only-summary.

### Phase 6 - Quality and Rollout

- Add WER, latency, cost/minute, and user correction metrics.
- Add dictionary learning only from explicit user edits.
- Add enterprise deployment notes and DPIA checklist.

## Acceptance Gates

- `cloud_zdr` and `company_private` produce no durable raw audio/transcript
  writes in database, object storage, search, logs, NATS, or caches.
- `company_private` works with provider egress blocked.
- Gateway rejects cloud voice calls when org policy disallows cloud processing.
- Teams import stores only user-approved summaries/tasks unless raw transcript
  storage is explicitly enabled.
- Every content-carrying route propagates `x-zdr`.
- Unit/integration coverage includes provider routing, policy denial, route
  envelopes, no-content logs, and ZDR cache bypass.
- E2E covers dictate, cancel, discard, insert, command preview, local-private
  mode, and Teams summary save.

## Open Decisions

1. Should private voice run as a Model Plane sidecar or a customer-managed
   connector?
2. Which customer plans get `company_private`?
3. Do we support raw transcript storage at all, or only summaries/tasks?
4. Do personal dictionaries sync across devices in ZDR mode?
5. Should Teams import require meeting-specific consent only, or also support
   tenant-wide admin consent for enterprise plans?
