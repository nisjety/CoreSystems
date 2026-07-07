# Voice

## Product role

Speech-in / speech-out interaction with the Model Plane. Supports push-to-talk,
continuous VAD-gated capture, and TTS playback of agent responses.

Velion v3 additionally needs a Wispr Flow-inspired dictation layer: fast
speech-to-text into composer/chat/inbox fields, AI cleanup, voice commands,
personal vocabulary, Teams transcript follow-up workflows, and enterprise
privacy controls. This is not a code or brand clone; it is a product pattern
adapted to CoreSystem plane boundaries.

Companion v3 plan:
`apps/Frontend Plane/velionv3/docs/voice-dictation-and-transcription-plan.md`.

## Transport

- **Capture**: client streams PCM/Opus frames over WebSocket to
  `model-gateway`.
- **Recognition**: `model-gateway` forwards to `inference-core`, which routes
  to the selected STT provider.
- **Synthesis**: text from `execution-core` → `inference-core` TTS route →
  audio frames streamed back to client.

## Backend ownership

| Concern | Owner |
|---------|-------|
| Audio ingress, codec negotiation | `model-gateway` |
| STT / TTS provider selection, streaming | `inference-core` |
| Turn management, barge-in | `execution-core` |
| Model catalog (STT/TTS capabilities) | `capability-core` |
| Voice org policy and retention defaults | Control Plane (`org-core`/`user-core`) |
| Microsoft Teams transcript acquisition | Ingestion Plane (`integration-corev2`) |
| Durable saved summaries/tasks | Data/Application Plane target service |

## Quality and retention modes

| Mode | Quality | Data boundary | Storage default |
|---|---:|---|---|
| `cloud_zdr` | High | Approved cloud STT/LLM processors | No raw audio/transcript/prompt/rewrite persistence |
| `company_private` | Medium | Customer VPC/on-prem/private cluster | No raw audio/transcript DB writes; memory/temp only |
| `browser_basic` | Variable | Browser/runtime behavior | Browser-only fallback; not a strong compliance mode |
| `saved_knowledge` | Depends | Same as selected processing mode until save | User-approved summary/task/note only |

The lower-quality option requested for regulated customers is `company_private`.
It should route to a self-hosted provider such as `whisper.cpp` first, with
`faster-whisper` as an accelerated deployment option and Vosk as a lightweight
fallback. The important invariant is that raw audio and raw transcripts do not
leave the customer-controlled environment and do not enter a company database
by default.

## `capability-core` responsibilities

- **Catalog**: enumerate STT/TTS models, languages, sample rates, streaming
  support, and latency class.
- **Policy**: per-org voice retention (e.g. do-not-store raw audio), allowed
  languages, PII redaction mode.
- **Metadata**: voice IDs, licensing constraints, regional availability.
- **Scheduling hints**: co-locate STT and main LLM call when possible.
- **Mode exposure**: publish available quality/retention modes to the gateway
  so Velion can show truthful UI labels.

## ZDR and persistence rules

- Raw audio must not be written to Postgres, Convex, Qdrant, Quickwit, MinIO,
  durable NATS payloads, or logs under `cloud_zdr` or `company_private`.
- Raw transcript must not be written to durable storage unless the user/org
  explicitly enables raw transcript retention.
- Prompt cache, inference cache, session transcript writes, and replay buffers
  must be bypassed for content-carrying voice requests when `zdr=true`.
- Audit records are metadata-only: request id, org id, user id, provider, mode,
  duration, and error category.
- Teams transcripts imported from Microsoft Graph are processed ephemerally by
  default; only approved summaries, tasks, notes, or CRM artifacts are saved.

## Reference inputs

- `hermes-agent` — voice channel integration patterns.
- Wispr Flow public docs — dictation, privacy controls, and enterprise policy
  patterns.
- Microsoft Graph Teams transcript APIs — post-meeting transcript import and
  change notifications.

## Out of scope

- On-device wake-word detection.
- Audio post-processing (EQ, noise suppression) — handled client-side.
- Copying Wispr Flow code, visual identity, branding, or private UX assets.
