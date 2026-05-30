# Voice

## Product role

Speech-in / speech-out interaction with the Model Plane. Supports push-to-talk,
continuous VAD-gated capture, and TTS playback of agent responses.

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

## `capability-core` responsibilities

- **Catalog**: enumerate STT/TTS models, languages, sample rates, streaming
  support, and latency class.
- **Policy**: per-org voice retention (e.g. do-not-store raw audio), allowed
  languages, PII redaction mode.
- **Metadata**: voice IDs, licensing constraints, regional availability.
- **Scheduling hints**: co-locate STT and main LLM call when possible.

## Reference inputs

- `hermes-agent` — voice channel integration patterns.

## Out of scope

- On-device wake-word detection.
- Audio post-processing (EQ, noise suppression) — handled client-side.
